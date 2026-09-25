//! The a4norm command line: the same flags, the same report, the same files
//! as the Python script it replaces.

use a4norm_rs::{basename, encode_page, io, parse_args, run, Fail, Opts, Source};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// A4MEM=1: the allocator counts live bytes, and each stage prints the
/// peak since the one before -- what the browser's memory has to hold.
mod mem {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    pub static LIVE: AtomicUsize = AtomicUsize::new(0);
    pub static PEAK: AtomicUsize = AtomicUsize::new(0);
    pub struct Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            let n = LIVE.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(n, Relaxed);
            System.alloc(l)
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            LIVE.fetch_sub(l.size(), Relaxed);
            System.dealloc(p, l)
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            if n > l.size() {
                let v = LIVE.fetch_add(n - l.size(), Relaxed) + n - l.size();
                PEAK.fetch_max(v, Relaxed);
            } else {
                LIVE.fetch_sub(l.size() - n, Relaxed);
            }
            System.realloc(p, l, n)
        }
    }
    pub fn report(stage: &str) {
        if std::env::var_os("A4MEM").is_some() {
            let mb = |v: usize| v as f64 / 1048576.0;
            eprintln!("mem {:10} live {:6.1} MB  peak {:6.1} MB", stage, mb(LIVE.load(Relaxed)), mb(PEAK.swap(LIVE.load(Relaxed), Relaxed)));
        }
    }
}

#[global_allocator]
static ALLOC: mem::Counting = mem::Counting;

/// A line to stdout; a closed pipe (| head) is no error.
macro_rules! say {
    ($($a:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), $($a)*);
    }};
}

fn tool(cmd: &str, args: &[&str]) -> Result<Vec<u8>, Fail> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|_| Fail(format!("a4norm: required tool not found: {}", cmd)))?;
    if !out.status.success() {
        return Err(Fail(format!(
            "a4norm: {} failed: {}",
            cmd,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}

/// Page rasters of one input: a photo, or every page of a PDF.
fn rasterize(src: &str, work: &Path, tag: &str) -> Result<Vec<a4norm_rs::img::Src>, Fail> {
    let ext = Path::new(src).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext != "pdf" {
        let bytes = std::fs::read(src).map_err(|e| Fail(format!("a4norm: cannot read {}: {}", src, e)))?;
        return match io::decode(&bytes, src) {
            Ok(img) => Ok(vec![img]),
            // HEIC and the rest: whatever converter this machine has
            Err(e) => {
                let png = tool("magick", &[src, "-auto-orient", "png:-"])
                    .or_else(|_| {
                        let t = work.join(format!("{}-conv.png", tag));
                        tool("heif-convert", &[src, &t.to_string_lossy()])?;
                        std::fs::read(&t).map_err(|e| Fail(e.to_string()))
                    })
                    .map_err(|_| e)?;
                Ok(vec![io::decode(&png, src)?])
            }
        };
    }
    let info = String::from_utf8_lossy(&tool("pdfinfo", &[src])?).to_string();
    let field = |k: &str| info.lines().find(|l| l.starts_with(k)).map(|l| l[k.len()..].trim().to_string());
    let pages: usize = field("Pages:").and_then(|v| v.parse().ok()).unwrap_or(0);
    let listing = String::from_utf8_lossy(&tool("pdfimages", &["-list", src])?).to_string();
    let n_img = listing.trim().lines().skip(2).count();
    if n_img == pages {
        let prefix = work.join(format!("{}-img", tag));
        tool("pdfimages", &["-all", src, &prefix.to_string_lossy()])?;
        let mut got: Vec<PathBuf> = std::fs::read_dir(work)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(&format!("{}-img-", tag)))
            .collect();
        got.sort();
        if got.len() == pages {
            return got
                .iter()
                .map(|p| {
                    let b = std::fs::read(p).map_err(|e| Fail(e.to_string()))?;
                    io::decode(&b, &p.to_string_lossy())
                })
                .collect();
        }
    }
    let w: f64 = field("Page size:").and_then(|v| v.split_whitespace().next().and_then(|x| x.parse().ok())).unwrap_or(595.0);
    let dpi = (2400.0 / (w / 72.0)).round().clamp(200.0, 400.0) as i64;
    let prefix = work.join(format!("{}-page", tag));
    tool("pdftoppm", &["-r", &dpi.to_string(), "-png", src, &prefix.to_string_lossy()])?;
    let mut got: Vec<PathBuf> = std::fs::read_dir(work)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_string();
            n.starts_with(&format!("{}-page", tag)) && n.ends_with(".png")
        })
        .collect();
    got.sort();
    got.iter()
        .map(|p| {
            let b = std::fs::read(p).map_err(|e| Fail(e.to_string()))?;
            io::decode(&b, &p.to_string_lossy())
        })
        .collect()
}

const SEG_PAPER_MIN: f64 = 40.0;

/// The script's segment_quad: the a4norm-seg helper's mask, judged by the
/// brightness detector's own rules.
fn segment_quad(img: &a4norm_rs::img::Src) -> (Option<a4norm_rs::detect::Quad>, String, f64) {
    use a4norm_rs::detect as d;
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("a4norm-seg")));
    let seg = std::env::var_os("PATH")
        .and_then(|p| std::env::split_paths(&p).map(|d| d.join("a4norm-seg")).find(|c| c.is_file()))
        .or_else(|| beside.filter(|b| b.is_file()));
    let seg = match seg {
        Some(s) => s,
        None => return (None, "no segmentation helper installed".into(), 0.0),
    };
    let work = std::env::temp_dir().join(format!("a4norm-seg-{}", std::process::id()));
    std::fs::create_dir_all(&work).ok();
    let res = (|| {
        let photo = work.join("photo.png");
        let mask_png = work.join("mask.png");
        std::fs::write(&photo, a4norm_rs::io::png_rgb(&img.px, img.w, img.h)).ok();
        let out = match Command::new(&seg).arg(&photo).arg(&mask_png).output() {
            Ok(o) => o,
            Err(_) => return (None, "segmentation did not run (OSError)".into(), 0.0),
        };
        if !out.status.success() || !mask_png.exists() {
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            let tail = err.trim().lines().last().map(|l| format!(": {}", l)).unwrap_or_default();
            return (None, format!("segmentation failed{}", tail), 0.0);
        }
        let mask = match std::fs::read(&mask_png).ok().and_then(|b| a4norm_rs::io::decode(&b, "mask").ok()) {
            Some(m) => m,
            None => return (None, "segmentation failed".into(), 0.0),
        };
        let (w0, h0) = (img.w, img.h);
        let w = 400;
        let h = (a4norm_rs::ops::py_round(400.0 * h0 as f64 / w0 as f64)).max(1) as usize;
        let buf = d::raw_gray(&mask, w, h);
        let m: Vec<u8> = buf.iter().map(|&v| (v > 128) as u8).collect();
        let pm = d::Pm { mask: d::open4(&m, w, h), w, h, w0, h0 };
        let (quad, why, frac) = d::quad_from_mask(&pm);
        let quad = match quad {
            Some(q) => q,
            None => return (None, why, frac),
        };
        let (v, c, _) = d::raw_rgb(img, w, h);
        let mut vals: Vec<(u8, u8)> = (0..w * h).filter(|&i| pm.mask[i] != 0).map(|i| (v[i], c[i])).collect();
        if vals.is_empty() {
            return (None, "segmented region is empty".into(), frac);
        }
        let mut vs: Vec<u8> = vals.iter().map(|x| x.0).collect();
        vs.sort_unstable();
        let paper = vs[(0.95 * (vs.len() - 1) as f64) as usize] as f64;
        let thr = 60.0f64.max((paper * 0.72).floor());
        let share = vals.iter().filter(|(v, c)| *v as f64 >= thr && *c <= 45).count() as f64 / vals.len() as f64;
        vals.clear();
        if share * 100.0 < SEG_PAPER_MIN {
            return (
                None,
                format!("the segmented region does not look like paper ({} of it, under {:.0}%)", d::pc0(share), SEG_PAPER_MIN),
                frac,
            );
        }
        (Some(quad), why, frac)
    })();
    std::fs::remove_dir_all(&work).ok();
    res
}

fn default_out(src: &str, o: &Opts) -> String {
    let p = std::fs::canonicalize(src).unwrap_or_else(|_| PathBuf::from(src));
    let stem = p.file_stem().unwrap().to_string_lossy().to_string();
    let ext = if o.format == "pdf" { ".pdf" } else { ".jpg" };
    p.with_file_name(format!("{}-A4{}", stem, ext)).to_string_lossy().to_string()
}

fn build(group: &[String], out: &str, o: &Opts) -> Result<Vec<String>, Fail> {
    let work = std::env::temp_dir().join(format!("a4norm-{}-{}", std::process::id(), group.len()));
    std::fs::create_dir_all(&work).ok();
    let res = (|| {
        let mut sources = vec![];
        for (si, s) in group.iter().enumerate() {
            let rasters = rasterize(s, &work, &format!("s{:02}", si))?;
            // A4DUMP=file.rgb: the first raster as decoded, for comparing decoders
            if let (Ok(p), Some(r)) = (std::env::var("A4DUMP"), rasters.first()) {
                std::fs::write(&p, &r.px).ok();
                eprintln!("dumped {}x{}", r.w, r.h);
            }
            sources.push(Source { name: s.clone(), rasters });
        }
        mem::report("decode");
        let pages = run(sources, o, &mut |l| say!("{}", l), &|st, _| mem::report(st))?;
        if o.dry_run {
            say!("  (dry run — nothing written)");
            return Ok(vec![]);
        }
        let mut written = vec![];
        if o.format == "pdf" {
            let jpegs: Vec<Vec<u8>> = pages.iter().map(|p| encode_page(p, o)).collect();
            mem::report("encode");
            let dpis: Vec<usize> = pages.iter().map(|p| p.dpi).collect();
            std::fs::write(out, io::write_pdf(&jpegs, &dpis)).map_err(|e| Fail(format!("a4norm: {}: {}", out, e)))?;
            written.push(out.to_string());
        } else {
            let p = Path::new(out);
            let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_else(|| ".jpg".into());
            let stem = out.strip_suffix(&ext).unwrap_or(out).to_string();
            for (i, pg) in pages.iter().enumerate() {
                let dst = if pages.len() == 1 { out.to_string() } else { format!("{}-{}{}", stem, i + 1, ext) };
                std::fs::write(&dst, encode_page(pg, o)).map_err(|e| Fail(format!("a4norm: {}: {}", dst, e)))?;
                written.push(dst);
            }
        }
        if o.preview {
            let stem = match out.rfind('.') {
                Some(i) if !out[i..].contains('/') => &out[..i],
                _ => out,
            };
            std::fs::write(format!("{}-preview.png", stem), io::preview_png(&pages[0].img)).ok();
        }
        Ok(written)
    })();
    std::fs::remove_dir_all(&work).ok();
    res
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut o = match parse_args(&args) {
        Ok(o) => o,
        Err(Fail(m)) => {
            if m.contains("error:") {
                eprintln!("{}", m);
                return ExitCode::from(2);
            }
            say!("{}", m);
            return ExitCode::SUCCESS;
        }
    };
    o.seg = Some(segment_quad);
    for src in &o.inputs {
        if !Path::new(src).exists() {
            eprintln!("a4norm: no such file: {}", src);
            return ExitCode::from(1);
        }
    }
    let groups: Vec<Vec<String>> = if o.separate { o.inputs.iter().map(|s| vec![s.clone()]).collect() } else { vec![o.inputs.clone()] };
    for g in &groups {
        say!("{}", if g.len() > 1 { g.join(", ") } else { g[0].clone() });
        let out = match (&o.output, groups.len()) {
            (Some(p), 1) => p.clone(),
            _ => default_out(&g[0], &o),
        };
        match build(g, &out, &o) {
            Ok(written) => {
                for w in written {
                    let size = std::fs::metadata(&w).map(|m| m.len()).unwrap_or(0);
                    say!("  -> {}  ({:.0} KB)", w, size as f64 / 1024.0);
                }
            }
            Err(Fail(m)) => {
                eprintln!("{}", m);
                return ExitCode::from(1);
            }
        }
    }
    let _ = basename;
    ExitCode::SUCCESS
}
