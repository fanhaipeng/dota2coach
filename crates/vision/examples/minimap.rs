//! Runs the minimap reader over the saved frames of a match and checks it
//! against the GSI recording of the same match: is your own hero found where
//! GSI says it is, and how well do minimap pixels line up with world units.
//!
//! cargo run -p vision --example minimap -- games\frames\<match id> [games\<recording>.jsonl] [--marks <dir> <clock>...]
//!
//! `--marks` saves the given frames enlarged with the heroes found circled
//! (green allies, red enemies, white where GSI puts you), to check by eye.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use vision::minimap::{self, Point, Reader};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let frames = PathBuf::from(&args[0]);
    let match_id = frames.file_name().unwrap().to_string_lossy().into_owned();
    let recording = args
        .iter()
        .find(|a| a.ends_with(".jsonl"))
        .map(PathBuf::from)
        .or_else(|| find_recording(&frames, &match_id));
    let (marks_dir, marks): (Option<PathBuf>, Vec<i64>) =
        match args.iter().position(|a| a == "--marks") {
            Some(i) => (
                args.get(i + 1).map(PathBuf::from),
                args[i + 2..]
                    .iter()
                    .filter_map(|c| c.parse().ok())
                    .collect(),
            ),
            None => (None, Vec::new()),
        };
    let you = match &recording {
        Some(path) => load_positions(path)?,
        None => BTreeMap::new(),
    };

    let mut clocks: Vec<i64> = std::fs::read_dir(&frames)?
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix("-minimap.jpg")?
                .parse()
                .ok()
        })
        .collect();
    clocks.sort();

    let mut reader = Reader::default();
    let mut pairs = Vec::new(); // (GSI point, arrow pixel)
    let (mut alive_frames, mut found, mut enemies_total, mut allies_total) = (0, 0, 0, 0);
    let mut errors = Vec::new();
    for &clock in &clocks {
        let image = image::open(frames.join(format!("{clock:05}-minimap.jpg")))?.into_rgb8();
        let arrows = reader.arrows(&image);
        enemies_total += arrows.iter().filter(|a| !a.ally).count();
        allies_total += arrows.iter().filter(|a| a.ally).count();
        let gsi = you.get(&clock).copied();
        if let Some(p) = gsi {
            alive_frames += 1;
            let (ex, ey) = minimap::to_pixel(p);
            let nearest = arrows
                .iter()
                .filter(|a| a.ally)
                .map(|a| (a, (a.x - ex).hypot(a.y - ey)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((a, d)) = nearest
                && d < 12.0
            {
                found += 1;
                pairs.push((p, (a.x, a.y)));
                errors.push(d);
            }
        }
        if marks.contains(&clock)
            && let Some(dir) = &marks_dir
        {
            save_marks(
                &image,
                &arrows,
                gsi,
                &dir.join(format!("marks-{clock:05}.png")),
            )?;
            println!(
                "{} allies {:?} enemies {:?}",
                mmss(clock),
                arrows
                    .iter()
                    .filter(|a| a.ally)
                    .map(|a| (a.x as i32, a.y as i32))
                    .collect::<Vec<_>>(),
                arrows
                    .iter()
                    .filter(|a| !a.ally)
                    .map(|a| (a.x as i32, a.y as i32))
                    .collect::<Vec<_>>()
            );
        }
    }

    println!("frames read: {}", clocks.len());
    println!(
        "your hero alive in {alive_frames} frames; found within 12 px of GSI in {found} ({:.1}%)",
        found as f64 * 100.0 / alive_frames.max(1) as f64
    );
    errors.sort_by(f32::total_cmp);
    if !errors.is_empty() {
        let at = |q: f64| errors[((errors.len() - 1) as f64 * q) as usize];
        println!(
            "distance from GSI, pixels (1 px = {:.0} world units): median {:.1}, 90% {:.1}, max {:.1}",
            1.0 / 0.01365,
            at(0.5),
            at(0.9),
            at(1.0)
        );
    }
    println!(
        "heroes seen per frame on average: allies {:.2}, enemies {:.2}",
        allies_total as f64 / clocks.len() as f64,
        enemies_total as f64 / clocks.len() as f64
    );
    fit(&pairs);
    Ok(())
}

/// Least-squares fit of pixel = origin + scale * world on each axis.
fn fit(pairs: &[(Point, (f32, f32))]) {
    if pairs.len() < 10 {
        return;
    }
    let line = |xs: Vec<(f64, f64)>| {
        let n = xs.len() as f64;
        let (mx, my) = (
            xs.iter().map(|p| p.0).sum::<f64>() / n,
            xs.iter().map(|p| p.1).sum::<f64>() / n,
        );
        let sxy: f64 = xs.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
        let sxx: f64 = xs.iter().map(|p| (p.0 - mx).powi(2)).sum();
        let slope = sxy / sxx;
        let origin = my - slope * mx;
        let rms = (xs
            .iter()
            .map(|p| (p.1 - origin - slope * p.0).powi(2))
            .sum::<f64>()
            / n)
            .sqrt();
        (slope, origin, rms)
    };
    let (sx, ox, rx) = line(
        pairs
            .iter()
            .map(|(p, a)| (p.x as f64, a.0 as f64))
            .collect(),
    );
    let (sy, oy, ry) = line(
        pairs
            .iter()
            .map(|(p, a)| (p.y as f64, a.1 as f64))
            .collect(),
    );
    println!(
        "fit over {} frames: x = {ox:.1} + {sx:.5} * world_x (rms {rx:.1} px), y = {oy:.1} + {sy:.5} * world_y (rms {ry:.1} px)",
        pairs.len()
    );
}

fn find_recording(frames: &Path, match_id: &str) -> Option<PathBuf> {
    let games = frames.parent()?.parent()?;
    std::fs::read_dir(games)
        .ok()?
        .filter_map(|e| Some(e.ok()?.path()))
        .find(|p| p.to_string_lossy().ends_with(&format!("-{match_id}.jsonl")))
}

/// Your hero's position at each clock second while alive, from the first
/// payload with that clock.
fn load_positions(path: &Path) -> anyhow::Result<BTreeMap<i64, Point>> {
    let mut out = BTreeMap::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let Ok(line) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let p = &line["payload"];
        let hero = &p["hero"];
        let (Some(clock), Some(x), Some(y)) = (
            p["map"]["clock_time"].as_i64(),
            hero["xpos"].as_i64(),
            hero["ypos"].as_i64(),
        ) else {
            continue;
        };
        if hero["alive"].as_bool() == Some(true) {
            out.entry(clock).or_insert(Point { x, y });
        } else {
            out.remove(&clock);
        }
    }
    Ok(out)
}

fn save_marks(
    image: &image::RgbImage,
    arrows: &[minimap::Blob],
    you: Option<Point>,
    path: &Path,
) -> anyhow::Result<()> {
    const ZOOM: u32 = 3;
    let mut big = image::imageops::resize(
        image,
        image.width() * ZOOM,
        image.height() * ZOOM,
        image::imageops::FilterType::Nearest,
    );
    let mut ring = |x: f32, y: f32, r: f32, rgb: [u8; 3]| {
        for step in 0..120 {
            let a = step as f32 * std::f32::consts::TAU / 120.0;
            let (px, py) = (
                (x * ZOOM as f32 + r * a.cos()) as i64,
                (y * ZOOM as f32 + r * a.sin()) as i64,
            );
            if px >= 0 && py >= 0 && (px as u32) < big.width() && (py as u32) < big.height() {
                big.put_pixel(px as u32, py as u32, image::Rgb(rgb));
            }
        }
    };
    for a in arrows {
        let rgb = if a.ally { [0, 255, 0] } else { [255, 0, 0] };
        ring(a.x, a.y, 22.0, rgb);
        ring(a.x, a.y, 23.0, [0, 0, 0]);
    }
    if let Some(p) = you {
        let (x, y) = minimap::to_pixel(p);
        ring(x, y, 6.0, [255, 255, 255]);
    }
    big.save(path)?;
    Ok(())
}

fn mmss(clock: i64) -> String {
    format!("{}:{:02}", clock / 60, clock % 60)
}
