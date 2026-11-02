//! Replays a recorded match through the coach as `run --screen` would have
//! coached it, reading the top bar and the minimap from the frames saved
//! during the match (`run --frames 1`), and lists the calls made from the
//! screen.
//!
//! cargo run -p dota2coach --example calls -- [games\<recording>.jsonl] [--lang zh] [--all]

use std::collections::BTreeMap;
use std::path::PathBuf;

use coach::snapshot::GameSnapshot;
use dota2coach::{
    CoachConfig, MissingEnemies, Recording, apply_minimap, apply_top_bar, latest_recording,
    run_coach,
};
use tokio::sync::mpsc;
use vision::minimap::Minimap;
use vision::topbar::{Slot, TopBar};

struct Silent;
impl voice::Speaker for Silent {
    fn say(&self, _: &str) {}
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lang = args
        .iter()
        .position(|a| a == "--lang")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "en".to_owned());
    let all = args.iter().any(|a| a == "--all");
    let file = match args.iter().find(|a| a.ends_with(".jsonl")) {
        Some(file) => PathBuf::from(file),
        None => latest_recording(&PathBuf::from("games")).expect("no recordings in games"),
    };
    println!("recording: {}", file.display());

    // The frames sit in games/frames/<match id>, found from the first snapshot.
    let mut frames: BTreeMap<i64, (TopBar, Minimap)> = BTreeMap::new();
    let mut loaded = false;
    let mut missing = MissingEnemies::default();
    let mut shown: BTreeMap<i64, String> = BTreeMap::new();
    let mut observe = |s: &mut GameSnapshot| {
        if !loaded && !s.match_id.is_empty() {
            loaded = true;
            frames = read_frames(&file.with_file_name("frames").join(&s.match_id));
            println!("frames read: {}", frames.len());
        }
        if s.in_game
            && let Some((bar, map)) = frames.get(&s.clock_time)
        {
            apply_top_bar(s, bar);
            apply_minimap(s, map);
            missing.observe(s);
            shown.entry(s.clock_time).or_insert_with(|| {
                format!(
                    "top bar: {}\n        minimap: {} enemies seen, nearest {}, {} near, {} allies near, {} missing",
                    describe(bar),
                    s.enemies_seen,
                    s.nearest_enemy,
                    s.enemies_near,
                    s.allies_near,
                    s.enemies_missing
                )
            });
        }
    };

    let (tx, rx) = mpsc::channel(1024);
    let reader = {
        let file = file.clone();
        tokio::spawn(async move { gsi::record::replay(&file, 0.0, tx).await })
    };
    let config = CoachConfig {
        rules_dir: dota2coach::resolve_rules_dir(None),
        role: None,
        min_gap_secs: 3,
        lang,
    };
    let said = run_coach(rx, config, &Silent, Recording::Off, Some(&mut observe)).await?;
    reader.await??;

    let screen: Vec<_> = said
        .iter()
        .filter(|s| s.rule_id.starts_with("screen") || is_screen_rule(&s.rule_id))
        .collect();
    println!(
        "calls in all: {}; from the screen: {}",
        said.len(),
        screen.len()
    );
    for s in &said {
        let from_screen = screen.iter().any(|x| std::ptr::eq(*x, s));
        if !from_screen && !all {
            continue;
        }
        let seen = shown.get(&s.clock).cloned().unwrap_or_default();
        println!(
            "{:>6}  {:<28} {}{}",
            dota2coach::format_clock(s.clock),
            s.rule_id,
            s.text,
            if from_screen {
                format!("\n        {seen}")
            } else {
                String::new()
            }
        );
    }
    Ok(())
}

fn is_screen_rule(id: &str) -> bool {
    ["enemies_dead", "allies_dead", "gank", "enemies_missing"]
        .iter()
        .any(|r| id.contains(r))
}

/// Every frame's top bar and minimap, read in clock order so the minimap
/// reader learns the buildings as it would live.
fn read_frames(dir: &std::path::Path) -> BTreeMap<i64, (TopBar, Minimap)> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        eprintln!("no frames in {}", dir.display());
        return out;
    };
    let mut clocks: Vec<i64> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix("-topbar.jpg")?.parse().ok()
        })
        .collect();
    clocks.sort();
    let mut minimap = vision::minimap::Reader::default();
    for clock in clocks {
        let open = |part: &str| image::open(dir.join(format!("{clock:05}-{part}.jpg"))).ok();
        let (Some(bar), Some(map)) = (open("topbar"), open("minimap")) else {
            continue;
        };
        out.insert(
            clock,
            (
                vision::topbar::read(&bar.into_rgb8()),
                minimap.read(&map.into_rgb8()),
            ),
        );
    }
    out
}
/// For example `Radiant . . 24 . . | Dire . X . . .`.
fn describe(bar: &TopBar) -> String {
    let side = |slots: &[Slot; 5]| {
        slots
            .iter()
            .map(|s| match (s.dead, s.respawn) {
                (false, _) => ".".to_owned(),
                (true, Some(n)) => n.to_string(),
                (true, None) => "X".to_owned(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!("Radiant {} | Dire {}", side(&bar.radiant), side(&bar.dire))
}
