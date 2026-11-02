//! Reads the hero lineup from a match's saved top-bar frames and checks your
//! own hero against the GSI recording of the same match.
//!
//! cargo run -p vision --example lineup -- games\frames\<match id> [assets\heroes] [--minutes N]

use std::path::{Path, PathBuf};

use vision::heroes::{LineupReader, References};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let frames = PathBuf::from(&args[0]);
    let heroes = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("assets/heroes"));
    let minutes: i64 = args
        .iter()
        .position(|a| a == "--minutes")
        .and_then(|i| args.get(i + 1)?.parse().ok())
        .unwrap_or(5);
    let match_id = frames.file_name().unwrap().to_string_lossy().into_owned();
    let you = find_recording(&frames, &match_id)
        .map(|p| your_hero(&p))
        .transpose()?
        .flatten();

    let refs = References::load(&heroes)?;
    println!("references: {} heroes", refs.len());
    let mut clocks: Vec<i64> = std::fs::read_dir(&frames)?
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix("-topbar.jpg")?
                .parse()
                .ok()
        })
        .collect();
    clocks.sort();

    let mut reader = LineupReader::new(refs.clone());
    let (mut checked, mut right) = (0, 0);
    let mut sure_at = None;
    for &clock in &clocks {
        let image = image::open(frames.join(format!("{clock:05}-topbar.jpg")))?.into_rgb8();
        let bar = vision::topbar::read(&image);
        // Your own slot, every frame of the match.
        if let Some((name, slot)) = &you
            && let Some(read) = refs.read_frame(&image, &bar)[*slot]
        {
            checked += 1;
            right += (read == name) as u32;
        }
        if clock <= minutes * 60 {
            reader.observe(&image, &bar);
            if sure_at.is_none() && reader.lineup().is_some() {
                sure_at = Some(clock);
            }
        }
    }

    if let Some((name, slot)) = &you {
        println!(
            "your hero ({name}, slot {}): read right in {right} of {checked} alive frames ({:.1}%)",
            slot + 1,
            right as f64 * 100.0 / checked.max(1) as f64
        );
    }
    match sure_at {
        Some(clock) => println!("lineup sure at {}:{:02}", clock / 60, clock % 60),
        None => println!("lineup not sure within {minutes} minutes"),
    }
    for (i, (name, share, votes)) in reader.leaders().iter().enumerate() {
        println!(
            "  {} {}: {:<34} {:>5.1}% of {votes} votes",
            if i < 5 { "Radiant" } else { "Dire   " },
            i % 5 + 1,
            name.unwrap_or("-"),
            share * 100.0
        );
    }
    Ok(())
}

fn find_recording(frames: &Path, match_id: &str) -> Option<PathBuf> {
    let games = frames.parent()?.parent()?;
    std::fs::read_dir(games)
        .ok()?
        .filter_map(|e| Some(e.ok()?.path()))
        .find(|p| p.to_string_lossy().ends_with(&format!("-{match_id}.jsonl")))
}

/// Your hero's internal name and top-bar slot (0 to 9), from GSI.
fn your_hero(path: &Path) -> anyhow::Result<Option<(String, usize)>> {
    for line in std::fs::read_to_string(path)?.lines() {
        let Ok(line) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let p = &line["payload"];
        let (Some(name), Some(team), Some(slot)) = (
            p["hero"]["name"].as_str(),
            p["player"]["team_name"].as_str(),
            p["player"]["team_slot"].as_u64(),
        ) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let offset = if team == "dire" { 5 } else { 0 };
        return Ok(Some((name.to_owned(), offset + slot as usize)));
    }
    Ok(None)
}
