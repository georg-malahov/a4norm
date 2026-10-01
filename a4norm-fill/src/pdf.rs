//! The filled form as a PDF (A4Norm Forms E4): the answers as vector text in
//! Arimo, near black, over the form's own pages. The font is embedded as the
//! glyphs the answers use (a CID font with a ToUnicode map, so the text
//! copies as written), a few kilobytes.
//!
//! - A scan: each page its JPEG, full page, and the text over it.
//! - A PDF: a new PDF whose pages are the source's pages as Form XObjects,
//!   untouched, with the text over them. An encrypted source that opens
//!   without a password (AES with an empty user password and owner bans,
//!   as the Familienkasse's forms and Bavaria's Wohngeld) is decrypted; the
//!   copy is written unencrypted. The form's own fields (AcroForm widgets,
//!   XFA) are left out, so nothing of theirs shows over the text. When the
//!   source cannot be read, the caller's pictures of its pages stand in, as
//!   for a scan, and the result says so.

use crate::fill::{Layout, Mark};
use crate::font;
use lopdf::{dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::BTreeMap;
use subsetter::GlyphRemapper;

/// The PDF, and whether its pages are the source's pictures because the
/// source could not be read.
pub struct Output {
    pub pdf: Vec<u8>,
    pub fallback: bool,
}

/// The overlay's content for a page: the marks of `page`, drawn in points
/// from the page's top left, with `to_pdf` the matrix from there into the
/// page's space (it flips the y axis).
fn overlay(layout: &Layout, page: u32, to_pdf: [f32; 6], glyphs: &GlyphRemapper, pictures: &[Picture]) -> Vec<u8> {
    let [r, g, b] = layout.color;
    let mut s = format!("q {} cm {r:.3} {g:.3} {b:.3} rg {r:.3} {g:.3} {b:.3} RG 1 J\n", matrix(to_pdf));
    // pictures first, the answers over them; y runs down, so each is drawn
    // from its foot
    for p in pictures.iter().filter(|p| p.page == page) {
        let [x0, y0, x1, y1] = p.at;
        s += "q ";
        if p.angle != 0.0 {
            // turned about its middle, clockwise as y runs down
            let (sin, cos) = p.angle.sin_cos();
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            s += &format!("1 0 0 1 {cx:.3} {cy:.3} cm {cos:.5} {sin:.5} {:.5} {cos:.5} 0 0 cm 1 0 0 1 {:.3} {:.3} cm ", -sin, -cx, -cy);
        }
        s += &format!("{:.3} 0 0 {:.3} {x0:.3} {y1:.3} cm /{} Do Q\n", x1 - x0, -(y1 - y0), p.name);
    }
    for m in &layout.marks {
        match m {
            Mark::Text { page: p, x, y, size, angle, text } if *p == page => {
                let (sin, cos) = angle.sin_cos();
                // y runs down here: the glyphs' up is -y
                // two bytes a glyph: its number in the embedded subset
                let hex: String = text.chars().map(|c| format!("{:04X}", glyphs.get(font::glyph(c).0 .0).unwrap_or(0))).collect();
                s += &format!("BT /A4nFont {size:.2} Tf {cos:.5} {sin:.5} {sin:.5} {:.5} {x:.2} {y:.2} Tm <{hex}> Tj ET\n", -cos);
            }
            Mark::Cross { page: p, b, width } if *p == page => {
                s += &format!(
                    "{width:.2} w {:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} m {:.2} {:.2} l S\n",
                    b[0], b[1], b[2], b[3], b[2], b[1], b[0], b[3]
                );
            }
            _ => {}
        }
    }
    s += "Q\n";
    s.into_bytes()
}

fn matrix(m: [f32; 6]) -> String {
    m.iter().map(|v| format!("{v:.4}")).collect::<Vec<_>>().join(" ")
}

/// A picture set over a page: its XObject's name and object, where, and
/// turned how far clockwise (radians) about its middle.
struct Picture {
    page: u32,
    name: String,
    id: ObjectId,
    at: [f32; 4],
    angle: f32,
}

/// The pictures of `images` (PNG, JPEG) as image XObjects in `doc`, each
/// fitted into its place, its shape kept: RGB, and its alpha as a soft mask
/// when it has any, so a signature has no white around it.
fn pictures(doc: &mut Document, places: &[crate::fill::ImagePlace], images: &[&[u8]]) -> Result<Vec<Picture>, String> {
    if places.len() != images.len() {
        return Err(format!("{} image places and {} images", places.len(), images.len()));
    }
    let mut out = vec![];
    for (k, (place, bytes)) in places.iter().zip(images).enumerate() {
        let img = image::load_from_memory(bytes).map_err(|e| format!("image {}: {e}", k + 1))?.to_rgba8();
        let (w, h) = img.dimensions();
        let rgb: Vec<u8> = img.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
        let mut dict = dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        };
        if img.pixels().any(|p| p[3] < 255) {
            let mut mask = Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
                    "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
                },
                img.pixels().map(|p| p[3]).collect(),
            );
            let _ = mask.compress();
            dict.set("SMask", doc.add_object(mask));
        }
        let mut stream = Stream::new(dict, rgb);
        let _ = stream.compress();
        let id = doc.add_object(stream);
        // fitted into the box, in its middle
        let [x0, y0, x1, y1] = place.b;
        let s = ((x1 - x0) / w as f32).min((y1 - y0) / h as f32);
        let (pw, ph) = (w as f32 * s, h as f32 * s);
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let at = [cx - pw / 2.0, cy - ph / 2.0, cx + pw / 2.0, cy + ph / 2.0];
        let angle = place.rotate.filter(|d| d.rem_euclid(360.0) != 0.0).map_or(0.0, f32::to_radians);
        out.push(Picture { page: place.page, name: format!("A4nImage{k}"), id, at, angle });
    }
    Ok(out)
}

/// Arimo as the glyphs `layout` sets, embedded in `doc`: a Type0 font over a
/// CIDFontType2 with the subset as its FontFile2, the glyphs' widths, and a
/// ToUnicode map so the text copies as written. Also the map from the
/// font's glyph numbers to the subset's, which the text is written in.
fn embed_font(doc: &mut Document, layout: &Layout) -> Result<(ObjectId, GlyphRemapper), String> {
    let f = font::face();
    let mut glyphs = GlyphRemapper::new();
    // each glyph of the subset, and the character it stands for
    let mut unicode: BTreeMap<u16, char> = BTreeMap::new();
    for m in &layout.marks {
        if let Mark::Text { text, .. } = m {
            for c in text.chars() {
                let (g, lost) = font::glyph(c);
                let new = glyphs.remap(g.0);
                unicode.entry(new).or_insert(if lost { '?' } else { c });
            }
        }
    }
    let subset = subsetter::subset(font::DATA, 0, &glyphs).map_err(|e| format!("font subset: {e:?}"))?;
    let upm = f.units_per_em() as f32;
    let k = |v: i16| (v as f32 * 1000.0 / upm).round() as i64;
    // the subset's tag, six capitals before the name, from what it holds
    let mut h: u32 = 2166136261;
    for (g, c) in &unicode {
        h = (h ^ (*g as u32 ^ *c as u32)).wrapping_mul(16777619);
    }
    let tag: String = (0..6).map(|i| (b'A' + ((h >> (i * 5)) % 26) as u8) as char).collect();
    let name = format!("{tag}+Arimo-Regular").into_bytes();
    let mut file = Stream::new(dictionary! { "Length1" => subset.len() as i64 }, subset);
    let _ = file.compress();
    let file = doc.add_object(file);
    let bbox = f.global_bounding_box();
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor", "FontName" => Object::Name(name.clone()), "Flags" => 32,
        "FontBBox" => vec![k(bbox.x_min).into(), k(bbox.y_min).into(), k(bbox.x_max).into(), k(bbox.y_max).into()],
        "ItalicAngle" => 0, "Ascent" => k(f.ascender()), "Descent" => k(f.descender()),
        "CapHeight" => k(f.capital_height().unwrap_or(f.ascender())), "StemV" => 80, "FontFile2" => file,
    });
    // the subset's glyphs' widths, from glyph 0 on
    let widths: Vec<Object> =
        glyphs.remapped_gids().map(|o| Object::Integer(font::advance(ttf_parser::GlyphId(o)).round() as i64)).collect();
    let cid = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => Object::Name(name.clone()),
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::String(b"Adobe".to_vec(), StringFormat::Literal),
            "Ordering" => Object::String(b"Identity".to_vec(), StringFormat::Literal),
            "Supplement" => 0,
        },
        "FontDescriptor" => descriptor, "W" => vec![Object::Integer(0), Object::Array(widths)], "CIDToGIDMap" => "Identity",
    });
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def /CMapType 2 def\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
    );
    let pairs: Vec<(&u16, &char)> = unicode.iter().collect();
    for chunk in pairs.chunks(100) {
        cmap += &format!("{} beginbfchar\n", chunk.len());
        for (g, c) in chunk {
            let utf16: String = c.encode_utf16(&mut [0; 2]).iter().map(|u| format!("{u:04X}")).collect();
            cmap += &format!("<{g:04X}> <{utf16}>\n");
        }
        cmap += "endbfchar\n";
    }
    cmap += "endcmap CMapName currentdict /CMap defineresource pop end end\n";
    let mut to_unicode = Stream::new(dictionary! {}, cmap.into_bytes());
    let _ = to_unicode.compress();
    let to_unicode = doc.add_object(to_unicode);
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => Object::Name(name),
        "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Reference(cid)], "ToUnicode" => to_unicode,
    });
    Ok((font, glyphs))
}

/// A PDF of scanned pages: each a JPEG over its `PageInspection.sizePt`
/// (A4 when the page was not inspected), with its answers over it. A page
/// read turned (`orientation`) is shown turned so, the right way up.
pub fn scan(pages: &[&[u8]], sizes: &Sizes, layout: &Layout, images: &Images) -> Result<Vec<u8>, String> {
    let mut doc = Document::with_version("1.4");
    let pages_id = doc.new_object_id();
    let (font, glyphs) = embed_font(&mut doc, layout)?;
    let pics = pictures(&mut doc, images.0, images.1)?;
    let mut kids = vec![];
    for (i, jpeg) in pages.iter().enumerate() {
        let (w, h, comps) = jpeg_info(jpeg).ok_or(format!("page {}: not a JPEG", i + 1))?;
        let (size, turn) = sizes(i as u32 + 1).unwrap_or(([595.28, 841.89], 0));
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
                "ColorSpace" => if comps == 1 { "DeviceGray" } else { "DeviceRGB" },
                "BitsPerComponent" => 8, "Filter" => "DCTDecode",
            },
            jpeg.to_vec(),
        ));
        // the picture as it came, the page the right way up turned from it
        let [pw, ph] = if turn % 180 == 90 { [size[1], size[0]] } else { size };
        let mut content = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /A4nScan Do Q\n").into_bytes();
        content.extend(overlay(layout, i as u32 + 1, shown([0.0, 0.0, pw, ph], turn as i64, 1.0), &glyphs, &pics));
        let mut xobjects = dictionary! { "A4nScan" => image };
        for p in pics.iter().filter(|p| p.page == i as u32 + 1) {
            xobjects.set(p.name.clone(), p.id);
        }
        let mut stream = Stream::new(dictionary! {}, content);
        let _ = stream.compress();
        let contents = doc.add_object(stream);
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), pw.into(), ph.into()],
            "Rotate" => turn as i64,
            "Contents" => contents,
            "Resources" => dictionary! {
                "XObject" => xobjects,
                "Font" => dictionary! { "A4nFont" => font },
            },
        })));
    }
    let count = kids.len() as i64;
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    save(doc)
}

pub(crate) fn save(mut doc: Document) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    doc.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Width, height and components of a baseline or progressive JPEG.
pub(crate) fn jpeg_info(d: &[u8]) -> Option<(u32, u32, u8)> {
    if d.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    while i + 9 < d.len() {
        if d[i] != 0xFF {
            return None;
        }
        let marker = d[i + 1];
        let len = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let h = u16::from_be_bytes([d[i + 5], d[i + 6]]) as u32;
            let w = u16::from_be_bytes([d[i + 7], d[i + 8]]) as u32;
            return Some((w, h, d[i + 9]));
        }
        i += 2 + len;
    }
    None
}

/// A page's box: its own or inherited from the page tree.
pub(crate) fn inherited<'a>(doc: &'a Document, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut id = page;
    loop {
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(v) = d.get(key) {
            return Some(v);
        }
        id = d.get(b"Parent").and_then(Object::as_reference).ok()?;
    }
}

fn rect(doc: &Document, o: &Object) -> Option<[f32; 4]> {
    let o = match o {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        o => o,
    };
    let a = o.as_array().ok()?;
    let v: Vec<f32> = a.iter().filter_map(|x| x.as_float().ok().or(x.as_i64().ok().map(|i| i as f32))).collect();
    (v.len() == 4).then(|| [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

/// The appearances a page's annotations show (a form field's border, its
/// cells, a box), each with the matrix that puts it at its place: the
/// normal appearance (in its /AS state), its box mapped onto the
/// annotation's rectangle. Hidden ones, links and pop-ups are not shown.
fn appearances(doc: &Document, page: ObjectId) -> Vec<(ObjectId, [f32; 6])> {
    let resolve = |o: &Object| -> Option<Object> {
        match o {
            Object::Reference(id) => doc.get_object(*id).ok().cloned(),
            o => Some(o.clone()),
        }
    };
    let Some(annots) = doc.get_dictionary(page).ok().and_then(|d| d.get(b"Annots").ok()).and_then(resolve) else {
        return vec![];
    };
    let Ok(annots) = annots.as_array() else { return vec![] };
    let mut out = vec![];
    for a in annots {
        let Some(Object::Dictionary(a)) = resolve(a) else { continue };
        let kind = a.get(b"Subtype").and_then(Object::as_name).unwrap_or(b"");
        let flags = a.get(b"F").and_then(Object::as_i64).unwrap_or(0);
        if kind == b"Link" || kind == b"Popup" || flags & (2 | 32) != 0 {
            continue;
        }
        let Some(Object::Dictionary(ap)) = a.get(b"AP").ok().and_then(resolve) else { continue };
        let normal = match ap.get(b"N") {
            Ok(Object::Reference(id)) if doc.get_object(*id).is_ok_and(|o| o.as_stream().is_ok()) => Some(*id),
            Ok(n) => resolve(n).and_then(|n| {
                let states = n.as_dict().ok()?.clone();
                let state = a.get(b"AS").and_then(Object::as_name).ok()?;
                states.get(state).and_then(Object::as_reference).ok()
            }),
            Err(_) => None,
        };
        let Some(id) = normal else { continue };
        let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else { continue };
        let (Some(r), Some(bbox)) = (a.get(b"Rect").ok().and_then(|o| rect(doc, o)), stream.dict.get(b"BBox").ok().and_then(|o| rect(doc, o))) else {
            continue;
        };
        let m: Vec<f32> = stream
            .dict
            .get(b"Matrix")
            .and_then(Object::as_array)
            .map(|v| v.iter().filter_map(|x| x.as_float().ok().or(x.as_i64().ok().map(|i| i as f32))).collect())
            .unwrap_or_default();
        let m = if m.len() == 6 { [m[0], m[1], m[2], m[3], m[4], m[5]] } else { [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] };
        // the box as the matrix turns it, then stretched onto the rectangle
        let pts = [(bbox[0], bbox[1]), (bbox[2], bbox[1]), (bbox[0], bbox[3]), (bbox[2], bbox[3])]
            .map(|(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]));
        let (bx0, by0) = pts.iter().fold((f32::MAX, f32::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1)));
        let (bx1, by1) = pts.iter().fold((f32::MIN, f32::MIN), |a, p| (a.0.max(p.0), a.1.max(p.1)));
        if bx1 - bx0 <= 0.0 || by1 - by0 <= 0.0 {
            continue;
        }
        let (sx, sy) = ((r[2] - r[0]) / (bx1 - bx0), (r[3] - r[1]) / (by1 - by0));
        out.push((id, [sx, 0.0, 0.0, sy, r[0] - sx * bx0, r[1] - sy * by0]));
    }
    out
}

/// The request's picture places and the pictures' bytes, in that order.
pub type Images<'a> = (&'a [crate::fill::ImagePlace], &'a [&'a [u8]]);

/// A page's `sizePt` and `orientation` as it was inspected, by its number
/// from 1: the page the right way up, and how far the picture was turned
/// clockwise for that.
pub type Sizes<'a> = dyn Fn(u32) -> Option<([f32; 2], u16)> + 'a;

/// The matrix from points of the page as shown, from its top left, into
/// the space of a page whose `crop` box is shown turned by `rotate`, `k`
/// of its units to a point.
fn shown(crop: [f32; 4], rotate: i64, k: f32) -> [f32; 6] {
    let [cx0, cy0, cx1, cy1] = crop;
    match rotate {
        90 => [0.0, k, k, 0.0, cx0, cy0],
        180 => [-k, 0.0, 0.0, k, cx1, cy0],
        270 => [0.0, -k, -k, 0.0, cx1, cy1],
        _ => [k, 0.0, 0.0, -k, cx0, cy1],
    }
}

/// A new PDF: the source's pages as Form XObjects and the answers over
/// them. `sizes` are the pages' `sizePt` as they were inspected (the page as
/// shown: its crop box, turned by its /Rotate), to scale the answers from,
/// and their `orientation`: a page read turned is shown turned so, the
/// right way up.
pub fn over(source: &[u8], sizes: &Sizes, layout: &Layout, images: &Images) -> Result<Vec<u8>, String> {
    let mut doc = Document::load_mem(source).map_err(|e| e.to_string())?;
    if doc.is_encrypted() {
        return Err("encrypted, and it does not open without a password".into());
    }
    let (font, glyphs) = embed_font(&mut doc, layout)?;
    let pics = pictures(&mut doc, images.0, images.1)?;
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for (i, &page) in pages.iter().enumerate() {
        let media = inherited(&doc, page, b"MediaBox").and_then(|o| rect(&doc, o)).unwrap_or([0.0, 0.0, 595.28, 841.89]);
        let crop = inherited(&doc, page, b"CropBox").and_then(|o| rect(&doc, o)).unwrap_or(media);
        let inspected = sizes(i as u32 + 1);
        let turn = inspected.map_or(0, |s| s.1 as i64);
        let rotate = (inherited(&doc, page, b"Rotate").and_then(|o| o.as_i64().ok()).unwrap_or(0) + turn).rem_euclid(360);
        let resources = match inherited(&doc, page, b"Resources") {
            Some(Object::Reference(id)) => Object::Reference(*id),
            Some(o) => o.clone(),
            None => Object::Dictionary(dictionary! {}),
        };
        let content = doc.get_page_content(page);
        let mut form = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => media.iter().map(|&v| Object::Real(v)).collect::<Vec<_>>(),
                "Resources" => resources,
            },
            content,
        );
        let _ = form.compress();
        let form = doc.add_object(form);
        // the page as shown, in points from its top left, into its space
        let [cx0, cy0, cx1, cy1] = crop;
        let (w, h) = if rotate % 180 == 0 { (cx1 - cx0, cy1 - cy0) } else { (cy1 - cy0, cx1 - cx0) };
        let size = inspected.map_or([w, h], |s| s.0);
        let m = shown(crop, rotate, w / size[0].max(1.0));
        let mut body = b"q /A4nPage Do Q\n".to_vec();
        // what the form's own fields show (a comb's cells, a box's border)
        // is kept as drawn, the fields themselves are not
        let mut shown = lopdf::Dictionary::new();
        for (k, (ap, cm)) in appearances(&doc, page).into_iter().enumerate() {
            let name = format!("A4nAnnot{k}");
            body.extend(format!("q {} cm /{name} Do Q\n", matrix(cm)).into_bytes());
            shown.set(name, ap);
        }
        body.extend(overlay(layout, i as u32 + 1, m, &glyphs, &pics));
        for p in pics.iter().filter(|p| p.page == i as u32 + 1) {
            shown.set(p.name.clone(), p.id);
        }
        let mut stream = Stream::new(dictionary! {}, body);
        let _ = stream.compress();
        let contents = doc.add_object(stream);
        let d = doc.get_dictionary_mut(page).map_err(|e| e.to_string())?;
        d.set("Contents", contents);
        shown.set("A4nPage", form);
        d.set("Resources", dictionary! { "XObject" => shown, "Font" => dictionary! { "A4nFont" => font } });
        d.set("MediaBox", media.iter().map(|&v| Object::Real(v)).collect::<Vec<_>>());
        d.set("CropBox", crop.iter().map(|&v| Object::Real(v)).collect::<Vec<_>>());
        d.set("Rotate", rotate);
        // the form's own fields would show over the answers
        d.remove(b"Annots");
    }
    if let Ok(root) = doc.trailer.get(b"Root").and_then(Object::as_reference) {
        if let Ok(catalog) = doc.get_dictionary_mut(root) {
            for key in [&b"AcroForm"[..], b"Perms", b"OpenAction", b"AA", b"Names"] {
                catalog.remove(key);
            }
        }
    }
    doc.trailer.remove(b"Encrypt");
    doc.prune_objects();
    save(doc)
}

/// The filled form from a PDF: over its own pages, or, when it cannot be
/// read and `pages` (JPEG pictures of them, as they were inspected) are
/// given, over those. `images` are the bytes of the request's pictures.
pub fn fill_pdf(req: &crate::fill::Request, source: &[u8], pages: &[&[u8]], images: &[&[u8]]) -> Result<(Layout, Output), String> {
    let layout = crate::fill::layout(req);
    let sizes = |n| req.page_size(n);
    let imgs = (req.images.as_slice(), images);
    match over(source, &sizes, &layout, &imgs) {
        Ok(pdf) => Ok((layout, Output { pdf, fallback: false })),
        Err(e) if pages.is_empty() => Err(e),
        Err(_) => {
            let pdf = scan(pages, &sizes, &layout, &imgs)?;
            Ok((layout, Output { pdf, fallback: true }))
        }
    }
}

/// The filled form from scanned pages (JPEG), with the request's pictures.
pub fn fill_scan(req: &crate::fill::Request, pages: &[&[u8]], images: &[&[u8]]) -> Result<(Layout, Output), String> {
    let layout = crate::fill::layout(req);
    let pdf = scan(pages, &|n| req.page_size(n), &layout, &(req.images.as_slice(), images))?;
    Ok((layout, Output { pdf, fallback: false }))
}
