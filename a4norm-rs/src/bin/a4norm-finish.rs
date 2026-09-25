//! a4norm-finish IN.png OUT.png [--reps N] [--start N] [--stages PREFIX]
//!               [--compare PREFIX [--min-psnr DB]]
//!
//! Runs flat-field -> ink -> tone on an already flat page and prints the
//! time of each stage (best of N), decode and encode apart.
//!   --stages   write each stage's own output as PREFIX-{flat,neutral,tone}.png
//!   --compare  PSNR of each stage against the script's own PREFIX-*.png
//!              (bench/dump.py writes them); --min-psnr fails below it
//!   --start    1: IN is already flat-fielded, 2: already neutralized

#![allow(clippy::needless_range_loop)]

use a4norm_rs::{Params, Rgb};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (inp, out) = (&args[1], &args[2]);
    let opt = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .map(|i| args[i + 1].clone())
    };
    let reps: usize = opt("--reps").map_or(1, |v| v.parse().unwrap());
    let stages = opt("--stages");
    // --start 1: the input is already flat-fielded, 2: already neutralized
    let start: usize = opt("--start").map_or(0, |v| v.parse().unwrap());

    let t = Instant::now();
    let src = image::open(inp).expect("read").to_rgb8();
    let dec = t.elapsed().as_secs_f64();
    let (w, h) = (src.width() as usize, src.height() as usize);
    let p = Params::default();

    let mut best = [f64::MAX; 3];
    let mut share = 0.0;
    let mut img = Rgb::from_rgb8(&src, w, h);
    for _ in 0..reps {
        img = Rgb::from_rgb8(&src, w, h);
        let t0 = Instant::now();
        let clock = move || t0.elapsed().as_secs_f64();
        let mut ts = [0.0; 3];
        let mut last = clock();
        for st in start..3 {
            match st {
                0 => a4norm_rs::finish::flat_field(&mut img, &p),
                1 => share = a4norm_rs::finish::neutralize_ink(&mut img, &p),
                _ => a4norm_rs::finish::tone(&mut img, &p),
            }
            let t = clock();
            ts[st] = t - last;
            last = t;
        }
        for i in 0..3 {
            best[i] = best[i].min(ts[i]);
        }
    }
    let mut px = vec![0u8; w * h * 3];
    img.to_rgb8(&mut px);
    image::save_buffer(out, &px, w as u32, h as u32, image::ExtendedColorType::Rgb8).unwrap();
    let compare = opt("--compare");
    let min_psnr: f64 = opt("--min-psnr").map_or(0.0, |v| v.parse().unwrap());
    let mut worst = f64::INFINITY;
    if stages.is_some() || compare.is_some() {
        // each stage's own output, for comparing against ImageMagick's
        let mut img = Rgb::from_rgb8(&src, w, h);
        let mut save = |img: &Rgb, name: &str| {
            let mut px = vec![0u8; w * h * 3];
            img.to_rgb8(&mut px);
            if let Some(pre) = &stages {
                image::save_buffer(
                    format!("{pre}-{name}.png"),
                    &px,
                    w as u32,
                    h as u32,
                    image::ExtendedColorType::Rgb8,
                )
                .unwrap();
            }
            if let Some(pre) = &compare {
                let r = image::open(format!("{pre}-{name}.png"))
                    .expect("reference")
                    .to_rgb8();
                let db = psnr(&px, &r);
                println!("{name:8} PSNR against the script {db:.2} dB");
                worst = worst.min(db);
            }
        };
        a4norm_rs::finish::flat_field(&mut img, &p);
        save(&img, "flat");
        a4norm_rs::finish::neutralize_ink(&mut img, &p);
        save(&img, "neutral");
        a4norm_rs::finish::tone(&mut img, &p);
        save(&img, "tone");
    }
    println!(
        "{w}x{h}  decode {dec:.3}s  flat {:.3}s  neutral {:.3}s  tone {:.3}s  sum {:.3}s  coloured ink kept on {share:.3}%",
        best[0], best[1], best[2], best.iter().sum::<f64>()
    );
    if worst < min_psnr {
        eprintln!("a stage is {worst:.2} dB from the script, under {min_psnr} dB");
        std::process::exit(1);
    }
}

/// PSNR over all channels, as `magick compare -metric PSNR` reports it.
fn psnr(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(
        a.len(),
        b.len(),
        "the page and the reference differ in size"
    );
    let se: f64 = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| (x as f64 - y as f64).powi(2))
        .sum();
    let mse = se / a.len() as f64 / (255.0 * 255.0);
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (1.0 / mse).log10()
    }
}
