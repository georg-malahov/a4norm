//! Several PDFs and scanned pages as one PDF, the PDFs' pages as they are:
//! vector, their text selectable, nothing rasterized or compressed anew.
//!
//! Each source is read (one encrypted without a user password, as the
//! Familienkasse's forms, is decrypted), its objects renumbered after the
//! ones already taken, and the pages chosen hung, in order, under one page
//! tree; what a page inherited from its own tree (resources, boxes,
//! rotation) is set on it. A document's own form (AcroForm), outline and
//! names are left out: forms from several files would clash by their
//! fields' names. The fields' widgets stay on their pages as they look.
//! A scanned page is its JPEG, untouched, on a page of its size.

use crate::pdf::{inherited, jpeg_info, save};
use lopdf::{dictionary, Document, Object, ObjectId, Stream, StringFormat};

/// A part of the merged PDF.
pub enum Part<'a> {
    /// a PDF's pages: those numbered (from 1, in this order), or all
    Pdf { pdf: &'a [u8], pages: Option<Vec<u32>> },
    /// a scanned page (JPEG): `size_mm` when given, else its pixels at
    /// `dpi`, else A4 the picture's way round; turned `turn` quarters
    /// clockwise (/Rotate)
    Jpeg { jpg: &'a [u8], dpi: Option<f32>, turn: u8, size_mm: Option<[f32; 2]> },
}

/// The parts as one PDF, with `title` in its /Info when given.
pub fn merge(parts: &[Part], title: Option<&str>) -> Result<Vec<u8>, String> {
    let mut out = Document::with_version("1.7");
    let pages_id = out.new_object_id();
    let mut kids: Vec<Object> = vec![];
    for (k, part) in parts.iter().enumerate() {
        let n = k + 1;
        match part {
            Part::Pdf { pdf, pages } => {
                let mut doc = Document::load_mem(pdf).map_err(|e| format!("part {n}: {e}"))?;
                if doc.is_encrypted() {
                    return Err(format!("part {n}: encrypted, and it does not open without a password"));
                }
                doc.renumber_objects_with(out.max_id + 1);
                let all: Vec<ObjectId> = doc.get_pages().into_values().collect();
                let chosen: Vec<ObjectId> = match pages {
                    None => all.clone(),
                    Some(list) => list
                        .iter()
                        .map(|&p| all.get((p as usize).wrapping_sub(1)).copied().ok_or(format!("part {n}: no page {p} of {}", all.len())))
                        .collect::<Result<_, _>>()?,
                };
                // what each page inherits, set on it before its tree is left
                let keys: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
                let mut own = vec![];
                for &id in &chosen {
                    let got: Vec<(&[u8], Object)> = keys.iter().filter_map(|&key| inherited(&doc, id, key).map(|o| (key, o.clone()))).collect();
                    own.push((id, got));
                }
                // a page chosen twice is copied, so that each has its parent
                let mut seen = std::collections::HashSet::new();
                for (id, got) in own {
                    let mut page = doc.get_dictionary(id).map_err(|e| format!("part {n}: {e}"))?.clone();
                    for (key, o) in got {
                        page.set(key.to_vec(), o);
                    }
                    page.set("Parent", pages_id);
                    let target = if seen.insert(id) { id } else { doc.add_object(Object::Null) };
                    doc.objects.insert(target, Object::Dictionary(page));
                    kids.push(Object::Reference(target));
                }
                // the source's own catalog and tree go; everything else is
                // kept, and what nothing refers to is pruned below
                let max = doc.max_id;
                out.objects.extend(doc.objects.into_iter().filter(|(_, o)| {
                    !matches!(o, Object::Dictionary(d) if d.has_type(b"Catalog") || d.has_type(b"Pages"))
                }));
                out.max_id = out.max_id.max(max);
            }
            Part::Jpeg { jpg, dpi, turn, size_mm } => {
                let (w, h, comps) = jpeg_info(jpg).ok_or(format!("part {n}: not a JPEG"))?;
                let [pw, ph] = match (size_mm, dpi) {
                    (Some([a, b]), _) => [a * 72.0 / 25.4, b * 72.0 / 25.4],
                    (None, Some(d)) if *d > 0.0 => [w as f32 * 72.0 / d, h as f32 * 72.0 / d],
                    _ if w > h => [841.89, 595.28],
                    _ => [595.28, 841.89],
                };
                let image = out.add_object(Stream::new(
                    dictionary! {
                        "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
                        "ColorSpace" => if comps == 1 { "DeviceGray" } else { "DeviceRGB" },
                        "BitsPerComponent" => 8, "Filter" => "DCTDecode",
                    },
                    jpg.to_vec(),
                ));
                let contents = out.add_object(Stream::new(dictionary! {}, format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /A4nScan Do Q\n").into_bytes()));
                let page = out.add_object(dictionary! {
                    "Type" => "Page", "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), pw.into(), ph.into()],
                    "Rotate" => (*turn as i64 % 4) * 90,
                    "Contents" => contents,
                    "Resources" => dictionary! { "XObject" => dictionary! { "A4nScan" => image } },
                });
                kids.push(Object::Reference(page));
            }
        }
    }
    if kids.is_empty() {
        return Err("nothing to merge".into());
    }
    let count = kids.len() as i64;
    out.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }));
    let catalog = out.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    out.trailer.set("Root", catalog);
    if let Some(t) = title {
        let utf16: Vec<u8> = [0xFE, 0xFF].into_iter().chain(t.encode_utf16().flat_map(u16::to_be_bytes)).collect();
        let info = out.add_object(dictionary! { "Title" => Object::String(utf16, StringFormat::Hexadecimal) });
        out.trailer.set("Info", info);
    }
    out.prune_objects();
    out.compress();
    save(out)
}
