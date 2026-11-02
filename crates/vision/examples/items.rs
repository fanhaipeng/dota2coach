//! Reads your inventory from a match's saved item-panel frames and checks it
//! against the items GSI reported in each slot.
//!
//! cargo run -p vision --example items -- games\frames\<match id> [assets\items] [--fit]
//!
//! `--fit` searches for the slot grid that best matches GSI's items.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use vision::items::{Grid, INVENTORY, Icons};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let frames = PathBuf::from(&args[0]);
    let dir = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("assets/items"));
    let match_id = frames.file_name().unwrap().to_string_lossy().into_owned();
    let recording = find_recording(&frames, &match_id).expect("no recording for these frames");
    let truth = load_items(&recording)?;
    let icons = Icons::load(&dir)?;

    let mut clocks: Vec<i64> = truth
        .keys()
        .copied()
        .filter(|c| frames.join(format!("{c:05}-items.jpg")).exists())
        .collect();
    clocks.sort();
    let open = |clock: i64| -> anyhow::Result<image::RgbImage> {
        let image = image::open(frames.join(format!("{clock:05}-items.jpg")))?.into_rgb8();
        Ok(vision::items::at_detail(&image).into_owned())
    };

    if args.iter().any(|a| a == "--fit") {
        // A sample across the match, scored by how close each slot is to the
        // icon GSI says it holds.
        let sample: Vec<(i64, image::RgbImage)> = clocks
            .iter()
            .step_by((clocks.len() / 60).max(1))
            .filter_map(|&c| Some((c, open(c).ok()?)))
            .collect();
        let mut best = (f32::MAX, INVENTORY);
        for x in 728..=744 {
            for y in 50..=62 {
                for dx in [64, 65, 66, 67, 68, 69, 70] {
                    for dy in [45, 46, 47, 48, 49, 50, 51] {
                        let grid = Grid {
                            x,
                            y,
                            dx,
                            dy,
                            ..INVENTORY
                        };
                        let score = score(&icons, &grid, &sample, &truth);
                        if score < best.0 {
                            best = (score, grid);
                        }
                    }
                }
            }
        }
        println!("best grid: {:?} (score {:.1})", best.1, best.0);
        return Ok(());
    }

    let (mut checked, mut right) = (0, 0);
    let mut wrong: BTreeMap<(String, String), u32> = BTreeMap::new();
    for &clock in &clocks {
        let panel = open(clock)?;
        let read = icons.inventory(&panel);
        for (i, item) in truth[&clock].iter().enumerate() {
            checked += 1;
            if read[i] == item.as_deref() {
                right += 1;
            } else {
                let name = |n: Option<&str>| n.unwrap_or("empty").to_owned();
                *wrong
                    .entry((name(item.as_deref()), name(read[i])))
                    .or_default() += 1;
            }
        }
    }
    println!(
        "inventory slots read right (items and empty slots): {right} of {checked} ({:.1}%) over {} frames",
        right as f64 * 100.0 / checked.max(1) as f64,
        clocks.len()
    );
    let mut wrong: Vec<_> = wrong.into_iter().collect();
    wrong.sort_by_key(|a| std::cmp::Reverse(a.1));
    for ((was, read), n) in wrong.iter().take(15) {
        println!("  {was} read as {read}: {n} frames");
    }
    Ok(())
}

/// Mean distance from each held item's slot to that item's icon.
fn score(
    icons: &Icons,
    grid: &Grid,
    sample: &[(i64, image::RgbImage)],
    truth: &BTreeMap<i64, [Option<String>; 6]>,
) -> f32 {
    let (mut sum, mut n) = (0.0, 0);
    for (clock, panel) in sample {
        for (i, item) in truth[clock].iter().enumerate() {
            let Some(item) = item else { continue };
            let slot = grid.slot(panel, i as u32);
            if let Some(d) = icons.distance_to(&slot, item) {
                sum += d;
                n += 1;
            }
        }
    }
    sum / n.max(1) as f32
}

fn find_recording(frames: &Path, match_id: &str) -> Option<PathBuf> {
    let games = frames.parent()?.parent()?;
    std::fs::read_dir(games)
        .ok()?
        .filter_map(|e| Some(e.ok()?.path()))
        .find(|p| p.to_string_lossy().ends_with(&format!("-{match_id}.jsonl")))
}

/// GSI's slot0 to slot5 at each clock second, from the first payload.
fn load_items(path: &Path) -> anyhow::Result<BTreeMap<i64, [Option<String>; 6]>> {
    let mut out = BTreeMap::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let Ok(line) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let p = &line["payload"];
        let Some(clock) = p["map"]["clock_time"].as_i64() else {
            continue;
        };
        if out.contains_key(&clock) || !p["items"].is_object() {
            continue;
        }
        let slots = std::array::from_fn(|i| {
            p["items"][format!("slot{i}")]["name"]
                .as_str()
                .filter(|n| *n != "empty")
                .map(str::to_owned)
        });
        out.insert(clock, slots);
    }
    Ok(out)
}
