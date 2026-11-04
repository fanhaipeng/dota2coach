//! Replays a recorded match through the coach as `run --screen` would have
//! coached it, reading the top bar, the minimap and the item panel (with the
//! lineup and the selected hero's name) from the frames saved during the match
//! (`run --frames 1`). Lists the calls made from the screen and every read of
//! another hero's items.
//!
//! cargo run -p dota2coach --example calls -- [games\<recording>.jsonl] [--lang zh] [--all]

use std::collections::BTreeMap;
use std::path::PathBuf;

use coach::snapshot::GameSnapshot;
use dota2coach::{
    CoachConfig, HeroNames, MissingEnemies, PanelItems, PanelNamer, PanelRead, Recording,
    apply_lineup, apply_minimap, apply_top_bar, latest_recording, resolve_heroes_dir,
    resolve_items_dir, run_coach,
};
use tokio::sync::mpsc;
use vision::heroes::Lineup;
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
    let mut frames: BTreeMap<i64, Frame> = BTreeMap::new();
    let mut loaded = false;
    let mut missing = MissingEnemies::default();
    let mut panel_items = PanelItems::default();
    let mut roshan = dota2coach::RoshanTimer::default();
    let mut clicks: BTreeMap<i64, String> = BTreeMap::new();
    let mut shown: BTreeMap<i64, String> = BTreeMap::new();
    let mut observe = |s: &mut GameSnapshot| {
        if !loaded && !s.match_id.is_empty() {
            loaded = true;
            frames = read_frames(&file.with_file_name("frames").join(&s.match_id));
            println!("frames read: {}", frames.len());
        }
        if s.in_game
            && let Some(frame) = frames.get(&s.clock_time)
        {
            let (bar, map) = (&frame.bar, &frame.map);
            apply_top_bar(s, bar);
            apply_minimap(s, map);
            missing.observe(s);
            if let Some(lineup) = &frame.lineup {
                apply_lineup(s, lineup);
            }
            panel_items.observe(s, frame.panel.as_ref());
            s.quickbuy_set = frame.quickbuy;
            roshan.observe(s, frame.roshan);
            if (s.ally_items_age == 0 || s.enemy_items_age == 0)
                && let Some(p) = &frame.panel
            {
                clicks.entry(s.clock_time).or_insert_with(|| {
                    format!(
                        "{:?} {} items {:?}",
                        p.side,
                        p.hero.as_deref().unwrap_or("(name not read)"),
                        p.items.iter().flatten().collect::<Vec<_>>()
                    )
                });
            }
            shown.entry(s.clock_time).or_insert_with(|| {
                format!(
                    "top bar: {}\n        minimap: {} enemies seen, nearest {}, {} near, {} allies near, {} missing\n        stale enemy: {}, enemy items: {:?}",
                    describe(bar),
                    s.enemies_seen,
                    s.nearest_enemy,
                    s.enemies_near,
                    s.allies_near,
                    s.enemies_missing,
                    s.stale_enemy,
                    s.enemy_items
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

    println!("reads of another hero's panel: {} seconds", clicks.len());
    for (clock, read) in &clicks {
        println!("{:>6}  {read}", dota2coach::format_clock(*clock));
    }

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
    [
        "enemies_dead",
        "allies_dead",
        "gank",
        "enemies_missing",
        "check_items",
        "counters",
        "roshan",
    ]
    .iter()
    .any(|r| id.contains(r))
}

/// What the screen showed at one clock second.
struct Frame {
    bar: TopBar,
    map: Minimap,
    /// The lineup, once it would have been read live.
    lineup: Option<Lineup>,
    panel: Option<PanelRead>,
    /// An item queued for quick-buy, from the small screen copy.
    quickbuy: bool,
    /// Roshan's skull shows on the minimap.
    roshan: bool,
}

/// Every frame, read in clock order as live: the minimap reader learns the
/// buildings, the lineup is voted on until sure, and the panel's name is
/// only read again when its letters change.
fn read_frames(dir: &std::path::Path) -> BTreeMap<i64, Frame> {
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
    let refs = resolve_heroes_dir(None).and_then(|d| vision::heroes::References::load(&d).ok());
    let all_heroes: Vec<String> = refs.iter().flat_map(|r| r.names().to_vec()).collect();
    let mut lineup_reader = refs.map(vision::heroes::LineupReader::new);
    let mut lineup: Option<Lineup> = None;
    let icons = resolve_items_dir(None).and_then(|d| vision::items::Icons::load(&d).ok());
    let mut namer = HeroNames::load(&dota2coach::resolve_rules_dir(None))
        .ok()
        .map(PanelNamer::new);
    for clock in clocks {
        let open = |part: &str| image::open(dir.join(format!("{clock:05}-{part}.jpg"))).ok();
        let (Some(bar_image), Some(map)) = (open("topbar"), open("minimap")) else {
            continue;
        };
        let bar_image = bar_image.into_rgb8();
        let bar = vision::topbar::read(&bar_image);
        if lineup.is_none()
            && let Some(reader) = &mut lineup_reader
        {
            reader.observe(&bar_image, &bar);
            lineup = reader.lineup();
        }
        let panel = match (open("items"), &icons) {
            (Some(image), Some(icons)) => {
                let image = image.into_rgb8();
                let (known, outsiders) = dota2coach::panel_candidates(lineup.as_ref(), &all_heroes);
                Some(PanelRead {
                    side: vision::items::side(&image),
                    hero: namer
                        .as_mut()
                        .and_then(|n| n.name(&image, &known, &outsiders)),
                    items: icons.inventory(&image).map(|i| i.map(str::to_owned)),
                })
            }
            _ => None,
        };
        let quickbuy = open("screen").is_some_and(|screen| {
            let screen = screen.into_rgb8();
            let r = vision::quickbuy_area(screen.width(), screen.height());
            vision::quickbuy_set(
                &image::imageops::crop_imm(&screen, r.x, r.y, r.width, r.height).to_image(),
            )
        });
        out.insert(
            clock,
            Frame {
                bar,
                roshan: vision::minimap::roshan_shown(&map.to_rgb8()),
                map: minimap.read(&map.into_rgb8()),
                lineup: lineup.clone(),
                panel,
                quickbuy,
            },
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
