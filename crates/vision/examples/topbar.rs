//! Runs the top-bar reader over the saved frames of a match and checks it
//! against the GSI recording of the same match.
//!
//! cargo run -p vision --example topbar -- games\frames\<match id> [games\<recording>.jsonl] [--table] [--glyphs]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use image::RgbImage;
use vision::topbar::{self, TopBar};

/// What GSI says at one clock second, from the first payload with that clock.
#[derive(Debug, Clone, Copy)]
struct Truth {
    radiant_score: i64,
    dire_score: i64,
    alive: bool,
    respawn: i64,
    /// Our hero's side and place on the top bar.
    radiant: bool,
    team_slot: usize,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags: Vec<&str> = args
        .iter()
        .filter(|a| a.starts_with("--"))
        .map(|s| s.as_str())
        .collect();
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let frames = PathBuf::from(
        paths
            .first()
            .expect("usage: topbar <frames dir> [recording]"),
    );
    let match_id = frames.file_name().unwrap().to_string_lossy().into_owned();
    let recording = match paths.get(1) {
        Some(p) => Some(PathBuf::from(p)),
        None => find_recording(&frames, &match_id),
    };
    let truth = match &recording {
        Some(path) => load_truth(path)?,
        None => BTreeMap::new(),
    };

    let mut clocks: Vec<i64> = std::fs::read_dir(&frames)?
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            name.strip_suffix("-topbar.jpg")?.parse().ok()
        })
        .collect();
    clocks.sort();

    let mut reads = Vec::new();
    let (mut votes, mut counts) = ([[[0; 6]; 9]; 10], [0; 10]);
    for &clock in &clocks {
        let image =
            normalise(image::open(frames.join(format!("{clock:05}-topbar.jpg")))?.into_rgb8());
        if flags.contains(&"--glyphs") {
            learn_glyphs(&image, truth.get(&clock), &mut votes, &mut counts);
            continue;
        }
        let bar = topbar::read(&image);
        if flags.contains(&"--table") {
            println!("{}", row(clock, &bar));
        }
        reads.push((clock, bar));
    }
    if flags.contains(&"--glyphs") {
        print_digits(&votes, &counts);
    } else {
        report(&reads, &truth);
    }
    Ok(())
}

/// `games/<stamp>-<match id>.jsonl` next to `games/frames`.
fn find_recording(frames: &Path, match_id: &str) -> Option<PathBuf> {
    let games = frames.parent()?.parent()?;
    std::fs::read_dir(games)
        .ok()?
        .filter_map(|e| Some(e.ok()?.path()))
        .find(|p| p.to_string_lossy().ends_with(&format!("-{match_id}.jsonl")))
}

fn load_truth(path: &Path) -> anyhow::Result<BTreeMap<i64, Truth>> {
    let mut truth = BTreeMap::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let Ok(line) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let p = &line["payload"];
        let (Some(clock), Some(alive)) = (
            p["map"]["clock_time"].as_i64(),
            p["hero"]["alive"].as_bool(),
        ) else {
            continue;
        };
        truth.entry(clock).or_insert(Truth {
            radiant_score: p["map"]["radiant_score"].as_i64().unwrap_or(0),
            dire_score: p["map"]["dire_score"].as_i64().unwrap_or(0),
            alive,
            respawn: p["hero"]["respawn_seconds"].as_i64().unwrap_or(0),
            radiant: p["player"]["team_name"].as_str() != Some("dire"),
            team_slot: p["player"]["team_slot"].as_u64().unwrap_or(0) as usize,
        });
    }
    Ok(truth)
}

/// Frames saved before the crops were tightened hold the top bar at full
/// detail inside a wider band (1.3 by 0.09 of the screen height). Cut the
/// current crop out of those and scale it to 1080p detail.
fn normalise(image: RgbImage) -> RgbImage {
    let (w, h) = image.dimensions();
    if w * 100 / h <= 1350 {
        return image;
    }
    let screen = w * 10 / 13;
    let area = vision::top_bar_area(screen * 16 / 9, screen);
    let x = (w - area.width) / 2;
    let part = image::imageops::crop_imm(&image, x, 0, area.width, area.height).to_image();
    image::imageops::resize(
        &part,
        topbar::WIDTH,
        topbar::HEIGHT,
        image::imageops::FilterType::CatmullRom,
    )
}

fn our_slot(bar: &TopBar, t: &Truth) -> topbar::Slot {
    if t.radiant {
        bar.radiant[t.team_slot]
    } else {
        bar.dire[t.team_slot]
    }
}

/// Votes per digit and glyph cell, from our own hero's timer.
type Votes = [[[u32; 6]; 9]; 10];

/// Learns digit shapes from our own hero's timer: the screen shows one more
/// than GSI's `respawn_seconds` (it rounds up).
fn learn_glyphs(
    image: &RgbImage,
    truth: Option<&Truth>,
    votes: &mut Votes,
    counts: &mut [u32; 10],
) {
    let Some(t) = truth.filter(|t| !t.alive) else {
        return;
    };
    let x = topbar::slot_x(t.radiant, t.team_slot);
    let glyphs = topbar::timer_glyphs(image, x);
    let shown = (t.respawn + 1).to_string();
    if glyphs.len() != shown.len() {
        return;
    }
    for (g, d) in glyphs.iter().zip(shown.bytes()) {
        let d = (d - b'0') as usize;
        counts[d] += 1;
        for (y, row) in g.iter().enumerate() {
            for (x, &c) in row.iter().enumerate() {
                votes[d][y][x] += c as u32;
            }
        }
    }
}

fn print_digits(votes: &Votes, counts: &[u32; 10]) {
    for d in 0..10 {
        println!("    // {d}: {} samples", counts[d]);
        println!("    [");
        for row in &votes[d] {
            let line: String = row
                .iter()
                .map(|&v| char::from(b'0' + ((v + counts[d] / 2) / counts[d].max(1)) as u8))
                .collect();
            println!("        \"{line}\",");
        }
        println!("    ],");
    }
}

fn row(clock: i64, bar: &TopBar) -> String {
    let cell = |s: &topbar::Slot| match (s.dead, s.respawn) {
        (false, _) => "  . ".to_owned(),
        (true, Some(n)) => format!("{n:>3} "),
        (true, None) => "  X ".to_owned(),
    };
    let radiant: String = bar.radiant.iter().map(cell).collect();
    let dire: String = bar.dire.iter().map(cell).collect();
    format!("{:>2}:{:02} | {radiant}| {dire}", clock / 60, clock % 60)
}

fn report(reads: &[(i64, TopBar)], truth: &BTreeMap<i64, Truth>) {
    // Our own hero: dead or alive, and the timer, against GSI.
    let (mut checked, mut state_ok, mut dead, mut timer_read, mut timer_ok, mut timer_near) =
        (0, 0, 0, 0, 0, 0);
    let mut misses = Vec::new();
    for (clock, bar) in reads {
        let Some(t) = truth.get(clock) else { continue };
        let slot = our_slot(bar, t);
        checked += 1;
        if slot.dead == !t.alive {
            state_ok += 1;
        } else {
            misses.push(format!(
                "{} read {} gsi {}",
                mmss(*clock),
                if slot.dead { "dead" } else { "alive" },
                if t.alive { "alive" } else { "dead" }
            ));
        }
        if !t.alive {
            dead += 1;
            if let Some(n) = slot.respawn {
                timer_read += 1;
                // The screen rounds up: it shows one more than GSI.
                let diff = (n as i64 - (t.respawn + 1)).abs();
                timer_ok += (diff == 0) as u32;
                timer_near += (diff <= 1) as u32;
                if diff > 1 {
                    misses.push(format!("{} timer read {n} gsi {}", mmss(*clock), t.respawn));
                }
            } else {
                misses.push(format!(
                    "{} timer not read, gsi {}",
                    mmss(*clock),
                    t.respawn
                ));
            }
        }
    }
    println!("frames read: {}", reads.len());
    println!(
        "our hero, frames with GSI: {checked}; dead/alive right: {state_ok} ({:.2}%)",
        pct(state_ok, checked)
    );
    println!(
        "our hero dead in {dead} frames; timer read in {timer_read} ({:.1}%); exact {timer_ok}, within 1s {timer_near}",
        pct(timer_read, dead)
    );
    for m in misses.iter().take(40) {
        println!("  miss: {m}");
    }

    // Everyone: each kill in the GSI score should show as a new death on the
    // other side of the top bar within a few seconds. Deaths to creeps,
    // towers or neutrals, and denies, don't score, so some deaths have no kill.
    let mut onsets = Vec::new();
    for pair in reads.windows(2) {
        let [(_, before), (clock, now)] = pair else {
            unreachable!()
        };
        for (side, (n, b)) in [(&now.radiant, &before.radiant), (&now.dire, &before.dire)]
            .into_iter()
            .enumerate()
        {
            for (slot, (n, b)) in n.iter().zip(b).enumerate() {
                if n.dead && !b.dead {
                    onsets.push((*clock, side, slot, false));
                }
            }
        }
    }
    let mut kills = Vec::new();
    for ((_, before), (clock, now)) in truth.iter().zip(truth.iter().skip(1)) {
        // Dire kills are Radiant deaths (side 0), and the other way round.
        for _ in 0..(now.dire_score - before.dire_score).max(0) {
            kills.push((*clock, 0));
        }
        for _ in 0..(now.radiant_score - before.radiant_score).max(0) {
            kills.push((*clock, 1));
        }
    }
    let mut matched = 0;
    let mut unmatched_kills = Vec::new();
    for &(clock, side) in &kills {
        match onsets
            .iter_mut()
            .find(|(at, s, _, used)| !*used && *s == side && (clock - 3..=clock + 3).contains(at))
        {
            Some(onset) => {
                onset.3 = true;
                matched += 1;
            }
            None => unmatched_kills.push(format!("{} {}", mmss(clock), team(side))),
        }
    }
    println!(
        "kills in GSI: {}; matched by a death on the top bar within 3s: {matched} ({:.1}%)",
        kills.len(),
        pct(matched, kills.len() as u32)
    );
    if !unmatched_kills.is_empty() {
        println!("  kills with no death seen: {}", unmatched_kills.join(", "));
    }
    // Everyone's timers: GSI can't check other heroes, but a timer should
    // count down one a second.
    let (mut dead_frames, mut timers, mut steps, mut steady) = (0, 0, 0, 0);
    for pair in reads.windows(2) {
        let [(c0, before), (c1, now)] = pair else {
            unreachable!()
        };
        let slots = |bar: &TopBar| bar.radiant.into_iter().chain(bar.dire).collect::<Vec<_>>();
        for (b, n) in slots(before).into_iter().zip(slots(now)) {
            if !n.dead {
                continue;
            }
            dead_frames += 1;
            timers += n.respawn.is_some() as u32;
            if let (Some(b), Some(n)) = (b.respawn, n.respawn)
                && c1 - c0 == 1
            {
                steps += 1;
                // The clock and the timer tick apart, so a second can show the
                // same number twice or skip one.
                let ok = (0..=2).contains(&(b as i64 - n as i64));
                steady += ok as u32;
            }
        }
    }
    println!(
        "all heroes: dead in {dead_frames} hero-frames, timer read in {timers} ({:.1}%); second-to-second, {steady} of {steps} counted down by 0 to 2 ({:.1}%)",
        pct(timers, dead_frames),
        pct(steady, steps)
    );
    let extra: Vec<String> = onsets
        .iter()
        .filter(|o| !o.3)
        .map(|&(clock, side, slot, _)| format!("{} {} {}", mmss(clock), team(side), slot + 1))
        .collect();
    println!(
        "deaths on the top bar with no kill in the score: {}",
        extra.len()
    );
    if !extra.is_empty() {
        println!("  {}", extra.join(", "));
    }
}

fn mmss(clock: i64) -> String {
    format!("{}:{:02}", clock / 60, clock % 60)
}

fn team(side: usize) -> &'static str {
    if side == 0 { "Radiant" } else { "Dire" }
}
fn pct(a: u32, b: u32) -> f64 {
    if b == 0 {
        0.0
    } else {
        a as f64 * 100.0 / b as f64
    }
}
