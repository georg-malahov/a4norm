//! a4norm in Rust: photos of paper documents -> one scanner-grade A4 PDF.
//! The same crate builds the command line (Docker, the bot, the skill) and
//! the WebAssembly module the browser loads. The Python script it was
//! ported from is its oracle: the same decisions, the same report lines.

// Index loops read as the pixel arithmetic they are.
#![allow(clippy::needless_range_loop)]

pub mod detect;
pub mod finish;
pub mod img;
pub mod io;
pub mod ops;
pub mod page;
#[cfg(target_arch = "wasm32")]
pub mod wasm;

use img::Img;
use page::{Card, PageOut, Processed};

/// A run that must stop, with the message the script would exit with.
#[derive(Debug)]
pub struct Fail(pub String);

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The segmentation helper of the full image (a4norm-seg), when the host
/// has one: the photo -> (quad, why, share) by the brightness rules, or why not.
pub type SegFn = fn(&img::Src) -> (Option<detect::Quad>, String, f64);

/// Every flag of the script, with its default.
#[derive(Clone, Debug)]
pub struct Opts {
    pub seg: Option<SegFn>,
    pub inputs: Vec<String>,
    pub output: Option<String>,
    pub format: String,
    pub separate: bool,
    pub dpi: usize,
    pub dpi_given: bool,
    pub quality: u8,
    pub photo_quality: u8,
    pub photo_dpi: usize,
    pub gray: bool,
    pub landscape: bool,
    pub rotate: String,
    pub spread: String,
    pub cards: String,
    pub card_size: String,
    pub edges: String,
    pub rectify: String,
    pub photo: String,
    pub fit: String,
    pub margins: String,
    pub ml: f64,
    pub mr: f64,
    pub mt: f64,
    pub dry_run: bool,
    pub preview: bool,
    pub trim_band: f64,
    pub trim_step: f64,
    pub trim_pad: usize,
    pub trim_shave: f64,
    pub photo_paper: f64,
    pub rect_inset: f64,
    pub edge_band: f64,
    pub edge_keep: f64,
    pub band_structure: f64,
    pub band_dark: f64,
    pub no_edge_clean: bool,
    pub close: i64,
    pub bg_scale: f64,
    pub chroma: f64,
    pub ink_dark: f64,
    pub chroma_grow: i64,
    pub black_clip: f64,
    pub white_clip: f64,
    pub haze_min: f64,
    pub haze_std: f64,
    pub no_haze: bool,
    pub paper_thr: f64,
    pub sharpen: f64,
    pub content_min: f64,
    pub rotate_tol: f64,
    pub deskew_min: f64,
    pub no_trim: bool,
    pub no_deskew: bool,
    pub no_neutralize: bool,
    pub spread_scan: bool,
    pub no_despeckle: bool,
    pub no_keep_photo: bool,
    pub no_flatten_paper: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            seg: None,
            inputs: vec![],
            output: None,
            format: "pdf".into(),
            separate: false,
            dpi: 300,
            dpi_given: false,
            quality: 88,
            photo_quality: 82,
            photo_dpi: 200,
            gray: false,
            landscape: false,
            rotate: "auto".into(),
            spread: "auto".into(),
            cards: "auto".into(),
            card_size: "actual".into(),
            edges: "auto".into(),
            rectify: "auto".into(),
            photo: "auto".into(),
            fit: "auto".into(),
            margins: "30,15,20".into(),
            ml: 30.0,
            mr: 15.0,
            mt: 20.0,
            dry_run: false,
            preview: false,
            trim_band: 8.0,
            trim_step: 6.0,
            trim_pad: 6,
            trim_shave: 1.0,
            photo_paper: 20.0,
            rect_inset: 0.8,
            edge_band: 10.0,
            edge_keep: 1.0,
            band_structure: 6.0,
            band_dark: 60.0,
            no_edge_clean: false,
            close: 6,
            bg_scale: 6.0,
            chroma: 7.0,
            ink_dark: 85.0,
            chroma_grow: 6,
            black_clip: 0.1,
            white_clip: 2.0,
            haze_min: 83.0,
            haze_std: 2.0,
            no_haze: false,
            paper_thr: 80.0,
            sharpen: 0.55,
            content_min: 45.0,
            rotate_tol: 5.0,
            deskew_min: 0.4,
            no_trim: false,
            no_deskew: false,
            no_neutralize: false,
            spread_scan: false,
            no_despeckle: false,
            no_keep_photo: false,
            no_flatten_paper: false,
        }
    }
}

pub const USAGE: &str = "usage: a4norm [-h] [-o OUTPUT] [--format {pdf,jpg}] [--separate] [--dpi DPI]
              [--quality QUALITY] [--photo-quality PHOTO_QUALITY] [--photo-dpi PHOTO_DPI]
              [--gray] [--landscape] [--rotate {auto,0,90,180,270}] [--spread {auto,on,off}]
              [--cards {auto,off}] [--card-size {actual,fit}] [--edges {auto,off}]
              [--rectify {auto,on,off}] [--photo {auto,on,off}] [--fit {auto,edges,content,frame}]
              [--margins MARGINS] [--dry-run] [--preview] [tuning flags...]
              INPUT [INPUT ...]

Photo of a document -> scanner-like A4 PDF. The tuning flags are the Python
script's: --trim-band --trim-step --trim-pad --trim-shave --photo-paper
--rect-inset --edge-band --edge-keep --band-structure --band-dark
--no-edge-clean --close --bg-scale --chroma --ink-dark --chroma-grow
--black-clip --white-clip --haze-min --haze-std --no-haze --paper-thr
--sharpen --content-min --rotate-tol --deskew-min --no-trim --no-deskew
--no-neutralize --spread-scan --no-despeckle --no-keep-photo --no-flatten-paper";

/// The command line, as the script's argparse reads it.
pub fn parse_args(args: &[String]) -> Result<Opts, Fail> {
    let mut o = Opts::default();
    let err = |m: String| Fail(format!("{}\na4norm: error: {}", USAGE, m));
    let mut i = 0;
    let mut positional_only = false;
    type Set = fn(&mut Opts);
    let bools: &[(&str, Set)] = &[
        ("--separate", |o| o.separate = true),
        ("--gray", |o| o.gray = true),
        ("--landscape", |o| o.landscape = true),
        ("--dry-run", |o| o.dry_run = true),
        ("--preview", |o| o.preview = true),
        ("--no-edge-clean", |o| o.no_edge_clean = true),
        ("--no-haze", |o| o.no_haze = true),
        ("--no-trim", |o| o.no_trim = true),
        ("--no-deskew", |o| o.no_deskew = true),
        ("--no-neutralize", |o| o.no_neutralize = true),
        ("--spread-scan", |o| o.spread_scan = true),
        ("--no-despeckle", |o| o.no_despeckle = true),
        ("--no-keep-photo", |o| o.no_keep_photo = true),
        ("--no-flatten-paper", |o| o.no_flatten_paper = true),
    ];
    while i < args.len() {
        let a = &args[i];
        if positional_only || !a.starts_with('-') || a == "-" {
            o.inputs.push(a.clone());
            i += 1;
            continue;
        }
        if a == "--" {
            positional_only = true;
            i += 1;
            continue;
        }
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        if key == "-h" || key == "--help" {
            return Err(Fail(USAGE.to_string()));
        }
        if let Some((_, set)) = bools.iter().find(|(n, _)| *n == key) {
            set(&mut o);
            i += 1;
            continue;
        }
        let val = match inline {
            Some(v) => v,
            None => {
                i += 1;
                args.get(i).cloned().ok_or_else(|| err(format!("argument {}: expected one argument", key)))?
            }
        };
        i += 1;
        let f = |v: &str| v.parse::<f64>().map_err(|_| err(format!("argument {}: invalid float value: '{}'", key, v)));
        let n = |v: &str| v.parse::<i64>().map_err(|_| err(format!("argument {}: invalid int value: '{}'", key, v)));
        let choice = |v: &str, c: &[&str]| -> Result<String, Fail> {
            if c.contains(&v) {
                Ok(v.to_string())
            } else {
                Err(err(format!(
                    "argument {}: invalid choice: '{}' (choose from {})",
                    key,
                    v,
                    c.iter().map(|x| format!("'{}'", x)).collect::<Vec<_>>().join(", ")
                )))
            }
        };
        match key.as_str() {
            "-o" | "--output" => o.output = Some(val),
            "--format" => o.format = choice(&val, &["pdf", "jpg"])?,
            "--dpi" => {
                o.dpi = n(&val)?.max(1) as usize;
                o.dpi_given = true;
            }
            "--quality" => o.quality = n(&val)?.clamp(1, 100) as u8,
            "--photo-quality" => o.photo_quality = n(&val)?.clamp(1, 100) as u8,
            "--photo-dpi" => o.photo_dpi = n(&val)?.max(1) as usize,
            "--rotate" => o.rotate = choice(&val, &["auto", "0", "90", "180", "270"])?,
            "--spread" => o.spread = choice(&val, &["auto", "on", "off"])?,
            "--cards" => o.cards = choice(&val, &["auto", "off"])?,
            "--card-size" => o.card_size = choice(&val, &["actual", "fit"])?,
            "--edges" => o.edges = choice(&val, &["auto", "off"])?,
            "--rectify" => o.rectify = choice(&val, &["auto", "on", "off"])?,
            "--photo" => o.photo = choice(&val, &["auto", "on", "off"])?,
            "--fit" => o.fit = choice(&val, &["auto", "edges", "content", "frame"])?,
            "--margins" => o.margins = val,
            "--trim-band" => o.trim_band = f(&val)?,
            "--trim-step" => o.trim_step = f(&val)?,
            "--trim-pad" => o.trim_pad = n(&val)?.max(0) as usize,
            "--trim-shave" => o.trim_shave = f(&val)?,
            "--photo-paper" => o.photo_paper = f(&val)?,
            "--rect-inset" => o.rect_inset = f(&val)?,
            "--edge-band" => o.edge_band = f(&val)?,
            "--edge-keep" => o.edge_keep = f(&val)?,
            "--band-structure" => o.band_structure = f(&val)?,
            "--band-dark" => o.band_dark = f(&val)?,
            "--close" => o.close = n(&val)?,
            "--bg-scale" => o.bg_scale = f(&val)?,
            "--chroma" => o.chroma = f(&val)?,
            "--ink-dark" => o.ink_dark = f(&val)?,
            "--chroma-grow" => o.chroma_grow = n(&val)?,
            "--black-clip" => o.black_clip = f(&val)?,
            "--white-clip" => o.white_clip = f(&val)?,
            "--haze-min" => o.haze_min = f(&val)?,
            "--haze-std" => o.haze_std = f(&val)?,
            "--paper-thr" => o.paper_thr = f(&val)?,
            "--sharpen" => o.sharpen = f(&val)?,
            "--content-min" => o.content_min = f(&val)?,
            "--rotate-tol" => o.rotate_tol = f(&val)?,
            "--deskew-min" => o.deskew_min = f(&val)?,
            _ => return Err(err(format!("unrecognized arguments: {}", a))),
        }
    }
    if o.inputs.is_empty() {
        return Err(err("the following arguments are required: INPUT".into()));
    }
    let m: Vec<Option<f64>> = o.margins.split(',').map(|x| x.trim().parse().ok()).collect();
    match m.as_slice() {
        [Some(l), Some(r), Some(t)] => {
            o.ml = *l;
            o.mr = *r;
            o.mt = *t;
        }
        _ => return Err(Fail("a4norm: --margins wants three numbers, e.g. 30,15,20".into())),
    }
    Ok(o)
}

/// One finished page of the document.
pub struct Page {
    pub img: Img,
    pub photo: bool,
    pub dpi: usize,
}

/// A source: its name and its rasters (a photo is one; a PDF may be many).
pub struct Source {
    pub name: String,
    pub rasters: Vec<img::Src>,
}

/// Every raster of every source through the pipeline, the cards paired up,
/// one page per group -- the script's build() short of writing files.
/// `out` receives the lines the script prints; `progress` the share of the
/// whole run done, 0..1, rising.
pub fn run(sources: Vec<Source>, o: &Opts, out: &mut dyn FnMut(&str), progress: &dyn Fn(&str, f64)) -> Result<Vec<Page>, Fail> {
    struct Item {
        res: Processed,
        report: Vec<String>,
    }
    let total = sources.iter().map(|s| s.rasters.len()).sum::<usize>().max(1) as f64;
    let weight = |s: &str| match s {
        "locate" => 0.25,
        "rectify" => 0.35,
        "border" => 0.45,
        "flat" => 0.55,
        "ink" => 0.65,
        "tone" => 0.75,
        _ => 0.95,
    };
    let done = std::cell::Cell::new(0.0f64);
    let mut items: Vec<Item> = vec![];
    let mut k = 0usize;
    let n_src = sources.len();
    for (si, s) in sources.into_iter().enumerate() {
        if n_src > 1 {
            out(&format!("  [{}/{}] {}", si + 1, n_src, basename(&s.name)));
        }
        for r in s.rasters {
            let mut rep = vec![];
            let base = k as f64 / total;
            let step = |st: &str| {
                let v = (base + weight(st) / total).max(done.get());
                done.set(v);
                progress(st, v * 0.97);
            };
            let res = page::process_page(r, o, &mut rep, &step)?;
            items.push(Item { res, report: rep });
            k += 1;
        }
    }
    // a photo holding ONE card goes with the next one if that holds one too
    let one_card = |it: &Item| matches!(&it.res, Processed::Cards(c) if c.len() == 1);
    let mut groups: Vec<Vec<usize>> = vec![];
    let mut i = 0;
    while i < items.len() {
        if one_card(&items[i]) && i + 1 < items.len() && one_card(&items[i + 1]) {
            groups.push(vec![i, i + 1]);
            i += 2;
        } else {
            groups.push(vec![i]);
            i += 1;
        }
    }
    let mut pages = vec![];
    for (pno, g) in groups.iter().enumerate() {
        out(&format!("  page {}:", pno + 1));
        let first = &items[g[0]];
        if let Processed::Page(PageOut { img, photo, dpi }) = &first.res {
            for line in &first.report {
                out(&format!("    - {}", line));
            }
            pages.push(Page { img: img.clone(), photo: *photo, dpi: *dpi });
            continue;
        }
        let mut cards: Vec<&Card> = vec![];
        for &kk in g {
            if let Processed::Cards(c) = &items[kk].res {
                cards.extend(c.iter());
            }
        }
        // the front goes on top; with two fronts or none, input order
        cards.sort_by_key(|c| !c.front);
        let pg = page::card_page(&cards, o);
        for &kk in g {
            for line in &items[kk].report {
                out(&format!("    - {}", line));
            }
        }
        if g.len() > 1 {
            out(&format!("    - the cards of photos {} and {} laid out on one page, front above back", g[0] + 1, g[1] + 1));
        }
        let (pw, ph) = page::a4_px(o.dpi as f64);
        out(&format!(
            "    - page: {}x{}px @ {}dpi, cards at {}",
            pw,
            ph,
            o.dpi,
            if o.card_size == "fit" { "page width" } else { "real size" }
        ));
        pages.push(Page { img: pg, photo: false, dpi: o.dpi });
    }
    Ok(pages)
}

/// A page as the JPEG the script encodes: photos at --photo-quality with
/// 4:2:0, documents at --quality with full chroma unless written below --dpi.
pub fn encode_page(p: &Page, o: &Opts) -> Vec<u8> {
    let q = if p.photo { o.photo_quality } else { o.quality };
    let sub = p.photo || p.dpi < o.dpi;
    io::encode_jpeg(&p.img, q, sub, p.dpi)
}

pub fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}
