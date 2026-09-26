//! Pixels in and out: photos decoded with their EXIF turn applied, pages
//! encoded as JPEG, and the PDF written by hand, as the script writes it.

use crate::img::{Img, Src};
use crate::Fail;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::io::Cursor;

/// A photo's bytes -> the upright photo as RGB bytes, as `magick
/// -auto-orient` gives it.
pub fn decode(bytes: &[u8], name: &str) -> Result<Src, Fail> {
    let err = |e: &dyn std::fmt::Display| Fail(format!("a4norm: cannot read {}: {}", name, e));
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| err(&e))?;
    let mut dec = reader.into_decoder().map_err(|e| err(&e))?;
    let orient = dec.orientation().unwrap_or(Orientation::NoTransforms);
    let (w, h, px) = if bytes.starts_with(&[0xFF, 0xD8]) {
        drop(dec);
        jpeg(bytes).map_err(|e| err(&e))?
    } else {
        let rgb = DynamicImage::from_decoder(dec).map_err(|e| err(&e))?.into_rgb8();
        (rgb.width() as usize, rgb.height() as usize, rgb.into_raw())
    };
    Ok(orient_px(Src { w, h, px }, orient))
}

/// An EXIF turn applied to RGB bytes.
fn orient_px(s: Src, o: Orientation) -> Src {
    use Orientation::*;
    if o == NoTransforms {
        return s;
    }
    let (w, h) = (s.w, s.h);
    let swap = matches!(o, Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH);
    let (w2, h2) = if swap { (h, w) } else { (w, h) };
    let mut px = vec![0u8; s.px.len()];
    for y2 in 0..h2 {
        for x2 in 0..w2 {
            // the source pixel of output (x2, y2)
            let (x, y) = match o {
                Rotate90 => (y2, h - 1 - x2),
                Rotate180 => (w - 1 - x2, h - 1 - y2),
                Rotate270 => (w - 1 - y2, x2),
                FlipHorizontal => (w - 1 - x2, y2),
                FlipVertical => (x2, h - 1 - y2),
                Rotate90FlipH => (y2, x2),
                Rotate270FlipH => (w - 1 - y2, h - 1 - x2),
                NoTransforms => (x2, y2),
            };
            let (i, j) = ((y2 * w2 + x2) * 3, (y * w + x) * 3);
            px[i..i + 3].copy_from_slice(&s.px[j..j + 3]);
        }
    }
    Src { w: w2, h: h2, px }
}

/// A JPEG's pixels through jpeg-decoder, as RGB bytes.
fn jpeg(bytes: &[u8]) -> Result<(usize, usize, Vec<u8>), jpeg_decoder::Error> {
    use jpeg_decoder::PixelFormat;
    let mut d = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    let px = d.decode()?;
    let info = d.info().unwrap();
    let (w, h) = (info.width as u32, info.height as u32);
    let rgb: Vec<u8> = match info.pixel_format {
        PixelFormat::RGB24 => px,
        PixelFormat::L8 => px.iter().flat_map(|&v| [v, v, v]).collect(),
        PixelFormat::L16 => px.chunks(2).flat_map(|c| [c[0], c[0], c[0]]).collect(),
        PixelFormat::CMYK32 => px
            .chunks(4)
            .flat_map(|c| {
                let k = c[3] as u32;
                [(c[0] as u32 * k / 255) as u8, (c[1] as u32 * k / 255) as u8, (c[2] as u32 * k / 255) as u8]
            })
            .collect(),
    };
    Ok((w as usize, h as usize, rgb))
}

/// A page as JPEG: `-quality Q -sampling-factor 1x1|2x2 -strip`, grey when
/// every pixel is, with its dpi in the JFIF header.
pub fn encode_jpeg(img: &Img, quality: u8, subsample: bool, dpi: usize) -> Vec<u8> {
    use jpeg_encoder::{ColorType, Density, Encoder, SamplingFactor};
    let mut out = vec![];
    let gray = img.c.len() == 1 || (img.c[0].d == img.c[1].d && img.c[1].d == img.c[2].d);
    let mut enc = Encoder::new(&mut out, quality);
    enc.set_density(Density::Inch { x: dpi as u16, y: dpi as u16 });
    if gray {
        let px = img.c[0].bytes();
        enc.encode(&px, img.w as u16, img.h as u16, ColorType::Luma).expect("jpeg");
    } else {
        enc.set_sampling_factor(if subsample { SamplingFactor::F_2_2 } else { SamplingFactor::F_1_1 });
        let px = img.to_rgb8();
        enc.encode(&px, img.w as u16, img.h as u16, ColorType::Rgb).expect("jpeg");
    }
    out
}

/// Width, height and components of a JPEG, from its SOF marker.
fn jpeg_info(data: &[u8]) -> (usize, usize, usize) {
    let mut i = 2;
    while i + 9 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = data[i + 1];
        let len = ((data[i + 2] as usize) << 8) | data[i + 3] as usize;
        if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
            let h = ((data[i + 5] as usize) << 8) | data[i + 6] as usize;
            let w = ((data[i + 7] as usize) << 8) | data[i + 8] as usize;
            return (w, h, data[i + 9] as usize);
        }
        i += 2 + len;
    }
    (0, 0, 3)
}

/// A JPEG-per-page PDF, written by hand (the script's write_pdf).
pub fn write_pdf(jpegs: &[Vec<u8>], dpis: &[usize]) -> Vec<u8> {
    let mut objs: Vec<Vec<u8>> = vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(), vec![]];
    let mut kids = vec![];
    for (i, data) in jpegs.iter().enumerate() {
        let dpi = dpis[i] as f64;
        let (w, h, comps) = jpeg_info(data);
        let space: &[u8] = if comps == 1 { b"/DeviceGray" } else { b"/DeviceRGB" };
        let pw = w as f64 * 72.0 / dpi;
        let ph = h as f64 * 72.0 / dpi;
        let mut img = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace ", w, h).into_bytes();
        img.extend_from_slice(space);
        img.extend(format!(" /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", data.len()).bytes());
        img.extend_from_slice(data);
        img.extend_from_slice(b"\nendstream");
        objs.push(img);
        let img_id = objs.len();
        let content = format!("q {:.4} 0 0 {:.4} 0 0 cm /Im0 Do Q", pw, ph);
        objs.push(format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content).into_bytes());
        let cont_id = objs.len();
        objs.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.4} {:.4}] /Resources << /XObject << /Im0 {} 0 R >> /ProcSet [/PDF /ImageB /ImageC] >> /Contents {} 0 R >>",
                pw, ph, img_id, cont_id
            )
            .into_bytes(),
        );
        kids.push(objs.len());
    }
    objs[1] = format!(
        "<< /Type /Pages /Count {} /Kids [{}] >>",
        kids.len(),
        kids.iter().map(|k| format!("{} 0 R", k)).collect::<Vec<_>>().join(" ")
    )
    .into_bytes();
    let mut buf = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = vec![0usize; objs.len() + 1];
    for (i, body) in objs.iter().enumerate() {
        offsets[i + 1] = buf.len();
        buf.extend(format!("{} 0 obj\n", i + 1).bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref = buf.len();
    buf.extend(format!("xref\n0 {}\n", objs.len() + 1).bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        buf.extend(format!("{:010} 00000 n \n", off).bytes());
    }
    buf.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", objs.len() + 1, xref).bytes());
    buf
}

/// RGB bytes as a PNG.
pub fn png_rgb(px: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![];
    image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::NoFilter)
        .write_image(px, w as u32, h as u32, image::ExtendedColorType::Rgb8)
        .expect("png");
    out
}

/// A small PNG of a page, 900 px wide: `--preview`.
pub fn preview_png(img: &Img) -> Vec<u8> {
    let w = 900;
    let h = ((900.0 * img.h as f64 / img.w as f64) + 0.5).floor().max(1.0) as usize;
    let small = img.resize_auto(w, h).rgb().to_rgb8();
    let mut out = vec![];
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&small, w as u32, h as u32, image::ExtendedColorType::Rgb8)
        .expect("png");
    out
}

use image::ImageEncoder;
