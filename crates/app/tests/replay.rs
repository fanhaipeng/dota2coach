//! End to end: replay the sample recording through the real rules and check
//! what the coach says.

use std::path::PathBuf;
use std::sync::Mutex;

use dota2coach::{CoachConfig, Recording, run_coach};
use tokio::sync::mpsc;
use voice::Speaker;

#[derive(Default)]
struct Collect(Mutex<Vec<String>>);

impl Speaker for Collect {
    fn say(&self, text: &str) {
        self.0.lock().unwrap().push(text.to_owned());
    }
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

async fn replay_sample(role: Option<&str>, lang: &str) -> Vec<String> {
    let (tx, rx) = mpsc::channel(64);
    let file = repo().join("fixtures/sample_game.jsonl");
    let reader = tokio::spawn(async move { gsi::record::replay(&file, 0.0, tx).await });
    let speaker = Collect::default();
    let config = CoachConfig {
        rules_dir: repo().join("rules"),
        role: role.map(str::to_owned),
        min_gap_secs: 3,
        lang: lang.to_owned(),
    };
    let spoken: Vec<String> = run_coach(rx, config, &speaker, Recording::Off, None)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.text)
        .collect();
    assert_eq!(reader.await.unwrap().unwrap(), 184);
    assert_eq!(spoken, *speaker.0.lock().unwrap());
    spoken
}

#[tokio::test]
async fn coaches_the_sample_game() {
    let spoken = replay_sample(Some("pos1"), "en").await;
    let said = |text: &str| spoken.iter().any(|s| s == text);

    assert!(said("Water runes in 15 seconds."), "{spoken:#?}");
    assert!(said("You have no TP scroll."), "{spoken:#?}");
    // A carry near the river hears about power runes, not bounty runes.
    assert!(said("Power runes in 15 seconds."), "{spoken:#?}");
    assert!(said("Low health. Back off."), "{spoken:#?}");
    assert!(
        said("Laning is ending. Plan your farming route."),
        "{spoken:#?}"
    );
    assert!(said("You have 2400 gold. Go spend it."), "{spoken:#?}");
    assert!(
        said("Blink is done. Look for a Berserker's Call initiation."),
        "hero pack should load for Axe: {spoken:#?}"
    );

    // Danger outranks the role reminder that fires at the same moment.
    let pos = |text: &str| spoken.iter().position(|s| s == text).unwrap();
    assert!(pos("Low health. Back off.") < pos("Laning is ending. Plan your farming route."));
}

#[tokio::test]
async fn role_pack_only_loads_when_asked() {
    let spoken = replay_sample(None, "en").await;
    assert!(!spoken.iter().any(|s| s.starts_with("Laning is ending")));
}

#[tokio::test]
async fn coaches_the_sample_game_in_chinese() {
    let spoken = replay_sample(Some("pos1"), "zh").await;
    let said = |text: &str| spoken.iter().any(|s| s == text);

    assert!(said("水符15秒后刷新。"), "{spoken:#?}");
    assert!(said("能量符15秒后刷新。"), "{spoken:#?}");
    assert!(said("你身上没有回城卷轴。"), "{spoken:#?}");
    assert!(said("血量低，快撤退。"), "{spoken:#?}");
    assert!(said("对线期快结束了，规划好你的打钱路线。"), "{spoken:#?}");
    assert!(said("你有2400金钱，快去花掉。"), "{spoken:#?}");
    assert!(
        said("跳刀到手了，找机会用狂战士之吼先手开团。"),
        "hero pack messages should load for Axe: {spoken:#?}"
    );
    // Same rules, same timing: only the words change.
    assert_eq!(spoken.len(), replay_sample(Some("pos1"), "en").await.len());
}

/// Every message key a rule can use must exist in English (the fallback), and
/// every shipped language should translate all of them. Hero nicknames
/// (`[hero_alias]`) are each language's own.
#[test]
fn every_language_translates_every_message() {
    let root = repo().join("rules");
    let langs = coach::i18n::available(&root);
    assert!(langs.contains(&"en".to_owned()) && langs.contains(&"zh".to_owned()));
    for dir in lang_dirs(&root) {
        let keys = |lang: &str| -> Vec<String> {
            let text = std::fs::read_to_string(dir.join(format!("{lang}.toml"))).unwrap();
            let table: toml::Table = text.parse().unwrap();
            let mut out = Vec::new();
            flatten("", &table, &mut out);
            // Nicknames for recognising hero names differ by language.
            out.retain(|k| !k.starts_with("hero_alias."));
            out.sort();
            out
        };
        let en = keys("en");
        for lang in &langs {
            assert_eq!(keys(lang), en, "{lang} messages in {}", dir.display());
        }
    }
}

/// `rules/lang` plus every pack's `lang` folder.
fn lang_dirs(root: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.join("lang")];
    for kind in ["roles", "heroes"] {
        for pack in std::fs::read_dir(root.join(kind)).unwrap() {
            let dir = pack.unwrap().path().join("lang");
            if dir.is_dir() {
                dirs.push(dir);
            }
        }
    }
    dirs
}

/// Every hero pack loads, and none of its rules fail on a busy mid-game
/// moment, in any language.
#[test]
fn every_hero_pack_runs() {
    use coach::{Advisor, GameSnapshot, Packs, RuleEngine};
    let root = repo().join("rules");
    let snapshot = GameSnapshot {
        in_game: true,
        clock_time: 900,
        team: "radiant".into(),
        role: "pos1".into(),
        level: 14,
        alive: true,
        health_percent: 30,
        mana_percent: 10,
        ult_level: 2,
        ult_ready: true,
        gold: 2500,
        items: [
            "item_blink",
            "item_dragon_lance",
            "item_vladmir",
            "item_tpscroll",
        ]
        .map(String::from)
        .to_vec(),
        ..Default::default()
    };
    for hero in std::fs::read_dir(root.join("heroes")).unwrap() {
        let hero = hero.unwrap().file_name().to_string_lossy().into_owned();
        for lang in coach::i18n::available(&root) {
            let mut engine = RuleEngine::load(
                &root,
                Packs {
                    role: None,
                    hero: Some(hero.clone()),
                },
                &lang,
            )
            .unwrap();
            let said = engine.advise(&snapshot);
            let disabled: Vec<_> = engine.disabled().collect();
            assert!(disabled.is_empty(), "{hero} ({lang}): {disabled:?}");
            assert!(
                said.iter().any(|a| a.rule_id.starts_with("heroes/")),
                "{hero} ({lang}) said nothing of its own: {said:#?}"
            );
        }
    }
}

fn flatten(prefix: &str, table: &toml::Table, out: &mut Vec<String>) {
    for (key, value) in table {
        if key == "_meta" {
            continue;
        }
        let key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => flatten(&key, inner, out),
            _ => out.push(key),
        }
    }
}

/// Writes a recording of a laning stage spent at `pos`, one payload a second.
fn laning_recording(name: &str, team: &str, pos: (i64, i64)) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dota2coach-{name}-{}.jsonl", std::process::id()));
    let mut lines = String::new();
    for t in -10..=610 {
        let payload = serde_json::json!({
            "map": { "matchid": "1", "clock_time": t, "game_state": "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS" },
            "player": { "team_name": team, "last_hits": t / 10, "gold": 300 },
            "hero": { "name": "npc_dota_hero_slardar", "alive": true, "health_percent": 100,
                      "xpos": pos.0, "ypos": pos.1 },
            "items": { "teleport0": { "name": "item_tpscroll" } }
        });
        lines += &serde_json::json!({ "t_ms": (t + 10) * 1000, "payload": payload }).to_string();
        lines += "\n";
    }
    std::fs::write(&path, lines).unwrap();
    path
}

async fn replay_file(file: PathBuf, lang: &str) -> Vec<String> {
    let (tx, rx) = mpsc::channel(64);
    let reader = tokio::spawn(async move { gsi::record::replay(&file, 0.0, tx).await });
    let config = CoachConfig {
        rules_dir: repo().join("rules"),
        role: None,
        min_gap_secs: 3,
        lang: lang.to_owned(),
    };
    let spoken = run_coach(rx, config, &Collect::default(), Recording::Off, None)
        .await
        .unwrap();
    reader.await.unwrap().unwrap();
    spoken.into_iter().map(|s| s.text).collect()
}

#[tokio::test]
async fn detects_the_role_and_loads_its_pack() {
    // Dire offlane is the bottom lane.
    let spoken = replay_file(laning_recording("offlane", "dire", (4800, -6200)), "en").await;
    assert!(
        spoken
            .iter()
            .any(|s| s
                == "You seem to be playing offlaner, position 3, so I'll coach for that for now."),
        "{spoken:#?}"
    );
    // The detected pos3 pack is loaded, so its end-of-laning rule fires.
    assert!(
        spoken
            .iter()
            .any(|s| s.starts_with("Laning is ending. Look to pressure")),
        "{spoken:#?}"
    );
    assert_eq!(
        spoken
            .iter()
            .filter(|s| s.starts_with("You seem to be playing"))
            .count(),
        1
    );

    let spoken = replay_file(laning_recording("mid-zh", "radiant", (-1500, -1300)), "zh").await;
    assert!(
        spoken
            .iter()
            .any(|s| s == "你好像在打二号位中单，先按这个位置给建议。"),
        "{spoken:#?}"
    );
}

/// Over a struggling game (no farm, no wards, dying every minute), every
/// universal and role rule fires at least once and none fails, in every
/// language. Catches typos in field names and message keys.
#[test]
fn every_role_rule_fires_in_a_bad_game() {
    use coach::{Advisor, GameSnapshot, Packs, RuleEngine};
    use std::collections::BTreeSet;
    let root = repo().join("rules");
    for role in std::fs::read_dir(root.join("roles")).unwrap() {
        let role = role.unwrap().file_name().to_string_lossy().into_owned();
        for lang in coach::i18n::available(&root) {
            let mut engine = RuleEngine::load(
                &root,
                Packs {
                    role: Some(role.clone()),
                    hero: None,
                },
                &lang,
            )
            .unwrap();
            let mut fired = BTreeSet::new();
            for t in -30..1900 {
                let snapshot = GameSnapshot {
                    in_game: true,
                    clock_time: t,
                    role: role.clone(),
                    alive: t % 400 < 380,
                    health_percent: if t % 100 < 50 { 100 } else { 20 },
                    gold: 2500,
                    deaths: 5,
                    since_death: t.rem_euclid(60),
                    gold_at_death: 2000,
                    buyback_cost: 3000,
                    last_hits: 1,
                    gpm: 200,
                    // The top bar: some enemies dead, then most of your team.
                    screen: true,
                    enemies_dead: (t / 30).rem_euclid(4),
                    enemies_dead_long: (t / 30).rem_euclid(4),
                    dead_window: 30,
                    allies_dead: if t % 200 < 50 { 3 } else { 0 },
                    // The minimap: an enemy near you now and then, and enemies
                    // vanishing.
                    enemies_seen: 2,
                    enemies_near: if t % 90 < 30 { 2 } else { 0 },
                    enemies_missing: if t % 300 < 60 { 4 } else { 0 },
                    // Roshan dies every 10 minutes.
                    roshan_dead: true,
                    roshan_killed_at: t - t.rem_euclid(600),
                    // The lineup: an invisible hero and lots of magic.
                    enemy_heroes: ["riki", "lion", "lina", "zuus", "huskar"]
                        .map(|h| format!("npc_dota_hero_{h}"))
                        .to_vec(),
                    // The item panel: Lion's items not read for a while, and
                    // Huskar clicked now and then.
                    stale_enemy: "npc_dota_hero_lion".into(),
                    hero_items: [(
                        "npc_dota_hero_huskar".to_string(),
                        vec!["item_satanic".to_string(), "item_sphere".to_string()],
                    )]
                    .into(),
                    hero_items_age: [("npc_dota_hero_huskar".to_string(), t.rem_euclid(300))]
                        .into(),
                    ..Default::default()
                };
                for advice in engine.advise(&snapshot) {
                    assert!(!advice.message.contains('{'), "{lang}: {}", advice.message);
                    fired.insert(advice.rule_id);
                }
            }
            let disabled: Vec<_> = engine.disabled().collect();
            assert!(disabled.is_empty(), "{role} ({lang}): {disabled:?}");
            let silent: Vec<_> = engine
                .rule_ids()
                .filter(|id| !fired.contains(*id))
                .collect();
            assert!(silent.is_empty(), "{role} ({lang}) never fired: {silent:?}");
        }
    }
}

/// Two matches in one session are recorded to two files, and reviewing one
/// finds the death the recording contains.
#[tokio::test]
async fn records_each_match_and_reviews_it() {
    use dota2coach::{latest_recording, review_file};
    let dir = std::env::temp_dir().join(format!("dota2coach-games-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let mut lines = Vec::new();
    for (match_id, team) in [("111", "radiant"), ("222", "dire")] {
        for t in -5..=400 {
            // Dies at 5:00 with 1800 gold and drops to 1200.
            let alive = !(300..330).contains(&t);
            let gold = if t < 300 { 1800 } else { 1200 };
            lines.push(serde_json::json!({
                "map": { "matchid": match_id, "clock_time": t, "game_state": "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS" },
                "player": { "team_name": team, "gold": gold, "last_hits": t / 15 },
                "hero": { "name": "npc_dota_hero_sniper", "alive": alive, "health_percent": 100,
                          "xpos": -1500, "ypos": -1300 },
                "items": { "slot0": { "name": "item_dragon_lance" } }
            }));
        }
    }
    let (tx, rx) = mpsc::channel(64);
    let feeder = tokio::spawn(async move {
        for (i, payload) in lines.into_iter().enumerate() {
            let raw = payload.to_string();
            let state = serde_json::from_str(&raw).unwrap();
            let update = gsi::Update {
                received_ms: i as u64 * 100,
                raw,
                state,
            };
            tx.send(update).await.unwrap();
        }
    });
    let config = CoachConfig {
        rules_dir: repo().join("rules"),
        role: None,
        min_gap_secs: 3,
        lang: "en".into(),
    };
    run_coach(
        rx,
        config.clone(),
        &Collect::default(),
        Recording::per_match(&dir),
        None,
    )
    .await
    .unwrap();
    feeder.await.unwrap();

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(files[0].to_string_lossy().ends_with("-111.jsonl"));
    assert!(latest_recording(&dir).is_some());

    let report = review_file(&files[1], config).await.unwrap().unwrap();
    assert_eq!(report.match_id, "222");
    assert_eq!(report.hero, "sniper");
    assert_eq!(report.role, "pos2");
    assert_eq!(report.death_list.len(), 1);
    assert_eq!(
        (report.death_list[0].clock, report.death_list[0].gold_lost),
        (300, 600)
    );
    assert!(report.items.iter().any(|i| i.item == "item_dragon_lance"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A top-bar reading fills the screen fields from the player's side.
#[test]
fn reads_the_top_bar_from_the_players_side() {
    use coach::GameSnapshot;
    use dota2coach::apply_top_bar;
    use vision::topbar::{Slot, TopBar};

    let dead = |secs| Slot {
        dead: true,
        respawn: Some(secs),
    };
    let mut bar = TopBar::default();
    bar.radiant[0] = dead(45);
    bar.radiant[1] = dead(25);
    bar.radiant[2] = dead(8);
    bar.radiant[3] = Slot {
        dead: true,
        respawn: None,
    };
    bar.dire[4] = dead(30);

    let mut s = GameSnapshot {
        team: "dire".into(),
        alive: false,
        respawn_seconds: 29,
        ..Default::default()
    };
    apply_top_bar(&mut s, &bar);
    assert!(s.screen);
    assert_eq!(
        (s.enemies_dead, s.enemies_dead_long, s.dead_window),
        (4, 2, 25)
    );
    assert_eq!(s.allies_dead, 0, "the dead Dire hero is you");

    let mut s = GameSnapshot {
        team: "radiant".into(),
        alive: true,
        ..Default::default()
    };
    apply_top_bar(&mut s, &bar);
    assert_eq!(
        (s.enemies_dead, s.enemies_dead_long, s.dead_window),
        (1, 1, 30)
    );
    assert_eq!(s.allies_dead, 4);

    // Allies a few seconds from respawning don't count.
    bar.radiant[1] = dead(5);
    bar.radiant[2] = dead(1);
    apply_top_bar(&mut s, &bar);
    assert_eq!(s.allies_dead, 2, "45 seconds and no timer");

    let mut s = GameSnapshot::default();
    apply_top_bar(&mut s, &bar);
    assert!(!s.screen, "team not known yet");
}

#[test]
fn reads_the_minimap_around_you() {
    use coach::GameSnapshot;
    use dota2coach::{MissingEnemies, apply_minimap};
    use vision::minimap::{Minimap, Point};

    let p = |x, y| Point { x, y };
    let map = Minimap {
        // You, a teammate 2000 away and one 1000 away.
        allies: vec![p(-3010, -2990), p(-1000, -3000), p(-3000, -2000)],
        enemies: vec![p(-2000, -3000), p(3000, 3000)],
    };
    let mut s = GameSnapshot {
        alive: true,
        xpos: -3000,
        ypos: -3000,
        ..Default::default()
    };
    apply_minimap(&mut s, &map);
    assert!(s.screen);
    assert_eq!(
        (
            s.enemies_seen,
            s.nearest_enemy,
            s.enemies_near,
            s.allies_near
        ),
        (2, 1000, 1, 1)
    );
    assert_eq!(s.allies_close, 2, "the teammate 2000 away can still help");

    let alone = Minimap {
        allies: vec![p(-3000, -3000)],
        enemies: vec![],
    };
    apply_minimap(&mut s, &alone);
    assert_eq!(
        (
            s.enemies_seen,
            s.nearest_enemy,
            s.enemies_near,
            s.allies_near
        ),
        (0, -1, 0, 0)
    );

    // Three enemies seen, then one dies and the other two drop off the map.
    let mut missing = MissingEnemies::default();
    let mut at = |clock, seen, dead| {
        let mut s = GameSnapshot {
            clock_time: clock,
            enemies_seen: seen,
            enemies_dead: dead,
            ..Default::default()
        };
        missing.observe(&mut s);
        s.enemies_missing
    };
    assert_eq!(at(600, 3, 0), 0);
    assert_eq!(at(601, 2, 1), 0, "one died, two still seen");
    assert_eq!(at(605, 0, 1), 2);
    assert_eq!(at(630, 0, 1), 0, "too long ago to count");
}

/// Counter items depend on the enemy lineup, your role and what you own.
#[test]
fn suggests_counter_items_for_the_lineup() {
    use coach::{Advisor, GameSnapshot, Packs, RuleEngine};
    let root = repo().join("rules");
    let advise = |role: &str, enemies: &[&str], items: &[&str]| {
        let mut engine = RuleEngine::load(&root, Packs::default(), "en").unwrap();
        let snapshot = GameSnapshot {
            in_game: true,
            clock_time: 1300,
            role: role.into(),
            alive: false,
            items: items.iter().map(|i| i.to_string()).collect(),
            enemy_heroes: enemies
                .iter()
                .map(|h| format!("npc_dota_hero_{h}"))
                .collect(),
            ..Default::default()
        };
        engine
            .advise(&snapshot)
            .into_iter()
            .filter(|a| a.rule_id == "universal/counters")
            .map(|a| a.message)
            // The best tip; the rest are for when it was said recently.
            .take(1)
            .collect::<Vec<_>>()
    };
    let lineup = ["riki", "lion", "lina", "zuus", "phantom_assassin"];
    assert_eq!(
        advise("pos5", &lineup, &[]),
        ["They have Riki. Carry Dust and Sentry wards."]
    );
    assert_eq!(
        advise("pos1", &lineup, &["item_dust"]),
        ["Lots of magic and stuns on their side. Black King Bar next."]
    );
    assert_eq!(
        advise("pos1", &lineup, &["item_dust", "item_black_king_bar"]),
        ["Against Lion, get Linken's Sphere to block the lockdown."]
    );
    assert_eq!(
        advise(
            "pos1",
            &lineup,
            &["item_dust", "item_black_king_bar", "item_sphere"]
        ),
        ["Against Phantom Assassin, Monkey King Bar or Bloodthorn hits through evasion."]
    );
    assert!(advise("pos1", &[], &[]).is_empty(), "lineup not read yet");
}

#[test]
fn remembers_items_of_selected_heroes() {
    use coach::GameSnapshot;
    use dota2coach::{PanelItems, PanelRead};
    use vision::items::Side;

    let items = |names: &[&str]| -> [Option<String>; 6] {
        std::array::from_fn(|i| names.get(i).map(|n| n.to_string()))
    };
    let mut panel = PanelItems::default();
    let mut at = |clock, read: Option<PanelRead>| {
        let mut s = GameSnapshot {
            match_id: "1".into(),
            clock_time: clock,
            items: vec![
                "item_blink".into(),
                "item_tpscroll".into(),
                "item_arcane_boots".into(),
            ],
            enemy_heroes: vec![
                "npc_dota_hero_phantom_assassin".into(),
                "npc_dota_hero_lion".into(),
            ],
            ..Default::default()
        };
        panel.observe(&mut s, read.as_ref());
        s
    };
    let s = at(100, None);
    assert_eq!((s.enemy_items_age, s.ally_items_age), (-1, -1));

    // Your own panel, even if its bar looks red while you're dead.
    let s = at(
        110,
        Some(PanelRead {
            side: Some(Side::Enemy),
            hero: None,
            items: items(&["item_blink"]),
        }),
    );
    assert!(s.enemy_items.is_empty());
    // Your own panel with a name read wrong: a hero label next to the portrait.
    let s = at(
        112,
        Some(PanelRead {
            side: Some(Side::Enemy),
            hero: Some("npc_dota_hero_phantom_assassin".into()),
            items: items(&["item_blink", "item_arcane_boots", "item_tpscroll"]),
        }),
    );
    assert!(s.enemy_items.is_empty());
    assert!(s.hero_items.is_empty());

    let s = at(
        120,
        Some(PanelRead {
            side: Some(Side::Enemy),
            hero: Some("npc_dota_hero_phantom_assassin".into()),
            items: items(&["item_desolator", "item_power_treads"]),
        }),
    );
    assert_eq!(s.enemy_items, ["item_desolator", "item_power_treads"]);
    assert_eq!(s.enemy_items_age, 0);

    let s = at(
        200,
        Some(PanelRead {
            side: Some(Side::Ally),
            hero: None,
            items: items(&["item_ward_dispenser"]),
        }),
    );
    assert_eq!(s.ally_items, ["item_ward_dispenser"]);
    assert_eq!(s.enemy_items_age, 80);
    assert_eq!(
        s.hero_items["npc_dota_hero_phantom_assassin"],
        ["item_desolator", "item_power_treads"]
    );
    assert_eq!(s.hero_items_age["npc_dota_hero_phantom_assassin"], 80);
    assert_eq!(s.stale_enemy, "", "nothing 5 minutes old yet");
    let s = at(400, None);
    assert_eq!(s.stale_enemy, "npc_dota_hero_lion", "never read");
    let s = at(450, None);
    assert_eq!(
        s.stale_enemy, "npc_dota_hero_lion",
        "older than PA's read at 120"
    );
}

#[test]
fn recognises_hero_names_in_any_language() {
    use dota2coach::HeroNames;
    let names = HeroNames::load(&repo().join("rules")).unwrap();
    let lineup: Vec<String> = ["ursa", "lycan", "zuus", "vengefulspirit", "slardar"]
        .iter()
        .map(|h| format!("npc_dota_hero_{h}"))
        .collect();
    let read = |texts: &[&str]| {
        let texts: Vec<String> = texts.iter().map(|t| t.to_string()).collect();
        names.identify(&texts, &lineup).map(str::to_owned)
    };
    // As Windows text recognition read them, misreads included.
    assert_eq!(read(&["能 战 士"]).as_deref(), Some("npc_dota_hero_ursa"));
    assert_eq!(
        read(&["复 仇 之 7"]).as_deref(),
        Some("npc_dota_hero_vengefulspirit")
    );
    assert_eq!(read(&["ZEUS"]).as_deref(), Some("npc_dota_hero_zuus"));
    assert_eq!(read(&[""]), None);
    // Nicknames count too.
    let dire: Vec<String> = ["sven", "juggernaut", "tidehunter", "terrorblade"]
        .iter()
        .map(|h| format!("npc_dota_hero_{h}"))
        .collect();
    let read = |text: &str| names.identify(&[text.to_owned()], &dire).map(str::to_owned);
    assert_eq!(read("剑 圣").as_deref(), Some("npc_dota_hero_juggernaut"));
    assert_eq!(read("潮 汐").as_deref(), Some("npc_dota_hero_tidehunter"));
    let razor = [
        "npc_dota_hero_razor".to_owned(),
        "npc_dota_hero_sven".to_owned(),
    ];
    assert_eq!(
        names.identify(&["雷 泽".to_owned()], &razor),
        Some("npc_dota_hero_razor"),
        "Razor's name in the Chinese client"
    );
}

/// Items seen on an enemy count like the hero itself, and the coach asks for
/// a click on an enemy whose items are old, while it's safe to look.
#[test]
fn uses_the_items_read_on_enemies() {
    use coach::{Advisor, GameSnapshot, Packs, RuleEngine};
    let root = repo().join("rules");
    let mut engine = RuleEngine::load(&root, Packs::default(), "en").unwrap();
    let axe = "npc_dota_hero_axe".to_string();
    let mut s = GameSnapshot {
        in_game: true,
        clock_time: 1300,
        team: "radiant".into(),
        role: "pos5".into(),
        alive: true,
        gold: 1200,
        enemy_heroes: vec![axe.clone()],
        hero_items: [(axe.clone(), vec!["item_invis_sword".into()])].into(),
        stale_enemy: axe,
        ..Default::default()
    };
    let said = |engine: &mut RuleEngine, s: &GameSnapshot, rule: &str| {
        engine
            .advise(s)
            .into_iter()
            .filter(|a| a.rule_id == rule)
            .map(|a| a.message)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        said(&mut engine, &s, "universal/counters"),
        ["They have Axe. Carry Dust and Sentry wards."]
    );
    assert!(
        said(&mut engine, &s, "universal/check_items").is_empty(),
        "out on the map"
    );
    (s.xpos, s.ypos) = (-7000, -6600);
    assert_eq!(
        said(&mut engine, &s, "universal/check_items"),
        ["Click their Axe on the top bar to check the items."]
    );
}

#[test]
fn tells_the_counter_to_what_a_clicked_enemy_carries() {
    use coach::{Advisor, GameSnapshot, Packs, RuleEngine};
    let root = repo().join("rules");
    let h = |s: &str| format!("npc_dota_hero_{s}");
    let said = |role: &str, age: i64, items: &[&str]| {
        let mut engine = RuleEngine::load(&root, Packs::default(), "en").unwrap();
        let s = GameSnapshot {
            in_game: true,
            screen: true,
            clock_time: 1500,
            role: role.into(),
            alive: true,
            // Little gold: the quick-buy tray buys it.
            gold: 300,
            items: items.iter().map(|i| i.to_string()).collect(),
            enemy_heroes: vec![h("sven"), h("phantom_assassin"), h("lion")],
            hero_items: [
                (
                    h("sven"),
                    vec!["item_black_king_bar".into(), "item_satanic".into()],
                ),
                (
                    h("phantom_assassin"),
                    vec!["item_desolator".into(), "item_butterfly".into()],
                ),
            ]
            .into(),
            hero_items_age: [(h("sven"), age), (h("phantom_assassin"), age)].into(),
            ..Default::default()
        };
        engine
            .advise(&s)
            .into_iter()
            .filter(|a| a.rule_id == "universal/item_counters")
            .map(|a| a.message)
            // The best tip; the rest are for when it was said recently.
            .take(1)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        said("pos1", 0, &[]),
        ["Phantom Assassin has Butterfly. Monkey King Bar or Bloodthorn hits through it."]
    );
    assert_eq!(
        said("pos1", 5, &["item_monkey_king_bar"]),
        ["Sven heals a lot from items. Eye of Skadi or Shiva's Guard cuts it."]
    );
    assert_eq!(
        said("pos5", 10, &[]),
        ["Sven heals a lot from items. Spirit Vessel cuts it."]
    );
    assert!(said("pos1", 120, &[]).is_empty(), "not just read");
}

#[test]
fn tells_the_counter_to_orchid() {
    use coach::{Advisor, GameSnapshot, Packs, RuleEngine};
    let clinkz = "npc_dota_hero_clinkz".to_string();
    let said = |role: &str| {
        let mut engine = RuleEngine::load(repo().join("rules"), Packs::default(), "en").unwrap();
        let s = GameSnapshot {
            in_game: true,
            screen: true,
            clock_time: 660,
            role: role.into(),
            alive: true,
            enemy_heroes: vec![clinkz.clone()],
            hero_items: [(
                clinkz.clone(),
                vec!["item_orchid".into(), "item_power_treads".into()],
            )]
            .into(),
            hero_items_age: [(clinkz.clone(), 2)].into(),
            ..Default::default()
        };
        engine
            .advise(&s)
            .into_iter()
            .filter(|a| a.rule_id == "universal/item_counters")
            .map(|a| a.message)
            // The best tip; the rest are for when it was said recently.
            .take(1)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        said("pos2"),
        ["Clinkz has Orchid. Manta Style or Black King Bar gets you out of the silence."]
    );
    assert_eq!(
        said("pos5"),
        ["Clinkz has Orchid. Eul's Scepter or Lotus Orb dispels the silence."]
    );
}

/// Rune reminders only for runes you can use: river runes when you're near
/// them, bounty runes and shrines for supports.
#[test]
fn reminds_of_runes_you_can_take() {
    use coach::{Advisor, GameSnapshot, Packs, RuleEngine};
    let root = repo().join("rules");
    let said = |role: &str, pos: (i64, i64), clock: i64| {
        let mut engine = RuleEngine::load(&root, Packs::default(), "en").unwrap();
        let s = GameSnapshot {
            in_game: true,
            clock_time: clock,
            role: role.into(),
            alive: true,
            xpos: pos.0,
            ypos: pos.1,
            items: vec!["item_tpscroll".into()],
            ..Default::default()
        };
        engine
            .advise(&s)
            .into_iter()
            .filter(|a| a.rule_id == "universal/runes")
            .map(|a| a.message)
            .collect::<Vec<_>>()
    };
    let mid = (-600, -400);
    let far_jungle = (-6000, -6000);
    assert_eq!(said("pos2", mid, 465), ["Power runes in 15 seconds."]);
    assert!(said("pos1", far_jungle, 465).is_empty());
    assert_eq!(
        said("pos5", far_jungle, 465),
        ["Bounty runes in 15 seconds."]
    );
    assert_eq!(
        said("pos4", far_jungle, 405),
        ["Wisdom shrines in 15 seconds."]
    );
    // 5:45: a lotus at 6:00, and power runes too.
    let lotus_pool = (-7000, 4000);
    assert_eq!(
        said("pos1", lotus_pool, 345),
        ["A healing lotus in 15 seconds at the pool near you."]
    );
    assert_eq!(said("pos1", mid, 345), ["Power runes in 15 seconds."]);
}

#[test]
fn learns_a_hero_the_lineup_missed_from_its_name() {
    use coach::GameSnapshot;
    use dota2coach::{PanelItems, PanelRead, panel_candidates};
    use vision::heroes::Lineup;
    use vision::items::Side;

    let h = |s: &str| format!("npc_dota_hero_{s}");
    // Anti-Mage with an alternative portrait: the lineup leaves the slot empty.
    let lineup = Lineup {
        radiant: [
            h("clinkz"),
            String::new(),
            h("ringmaster"),
            h("kunkka"),
            h("sniper"),
        ],
        dire: [
            h("ogre_magi"),
            h("sven"),
            h("necrolyte"),
            h("wisp"),
            h("juggernaut"),
        ],
    };
    let all = vec![h("antimage"), h("clinkz"), h("axe")];
    let (known, outsiders) = panel_candidates(Some(&lineup), &all);
    assert!(known.contains(&h("clinkz")) && !known.contains(&h("antimage")));
    assert_eq!(outsiders, [h("antimage"), h("axe")]);

    let mut panel = PanelItems::default();
    let mut s = GameSnapshot {
        match_id: "1".into(),
        team: "radiant".into(),
        hero: "sniper".into(),
        clock_time: 900,
        ..Default::default()
    };
    dota2coach::apply_lineup(&mut s, &lineup);
    assert_eq!(s.ally_heroes.len(), 4);
    let read = PanelRead {
        side: Some(Side::Ally),
        hero: Some(h("antimage")),
        items: std::array::from_fn(|i| (i == 0).then(|| "item_bfury".to_owned())),
    };
    panel.observe(&mut s, Some(&read));
    assert!(s.ally_heroes.contains(&h("antimage")));
    assert_eq!(s.hero_items[&h("antimage")], ["item_bfury"]);
}

/// The TP reminder: only when you can buy one, and not with Boots of Travel
/// or a TP in the stash.
#[test]
fn reminds_of_a_tp_only_when_it_helps() {
    use coach::{Advisor, GameSnapshot, Packs, RuleEngine};
    let root = repo().join("rules");
    let said = |gold: i64, items: &[&str], stash: &[&str]| {
        let mut engine = RuleEngine::load(&root, Packs::default(), "en").unwrap();
        let s = GameSnapshot {
            in_game: true,
            clock_time: 600,
            alive: true,
            gold,
            items: items.iter().map(|i| i.to_string()).collect(),
            stash: stash.iter().map(|i| i.to_string()).collect(),
            ..Default::default()
        };
        engine
            .advise(&s)
            .into_iter()
            .any(|a| a.rule_id == "universal/no_tp")
    };
    assert!(said(500, &[], &[]));
    assert!(!said(40, &[], &[]), "can't afford one");
    assert!(!said(500, &["item_travel_boots"], &[]));
    assert!(!said(500, &[], &["item_tpscroll"]));
}

#[test]
fn counts_down_to_roshan() {
    use coach::{Advisor, GameSnapshot, Packs, RuleEngine};
    let mut engine = RuleEngine::load(repo().join("rules"), Packs::default(), "en").unwrap();
    // Killed at 30:00. Cooldowns live in the coordinator, so keep when each
    // message first comes up.
    let said = |engine: &mut RuleEngine, clock: i64| {
        let s = GameSnapshot {
            in_game: true,
            clock_time: clock,
            alive: true,
            screen: true,
            roshan_dead: true,
            roshan_killed_at: 1800,
            ..Default::default()
        };
        engine
            .advise(&s)
            .into_iter()
            .find(|a| a.rule_id == "universal/roshan")
            .map(|a| a.message)
    };
    let mut calls: Vec<(i64, String)> = Vec::new();
    for t in 1800..2600 {
        if let Some(m) = said(&mut engine, t)
            && calls.last().is_none_or(|(_, last)| *last != m)
        {
            calls.push((t, m));
        }
    }
    assert_eq!(
        calls,
        [
            (
                1860,
                "Roshan is down. He's back between 38:00 and 41:00.".into()
            ),
            (
                2220,
                "Roshan can respawn in 1 minute, from 38:00. Get vision on his pit.".into()
            ),
            (
                2280,
                "Roshan can be back now. He is back by 41:00 at the latest.".into()
            ),
            (2400, "Roshan is back within 1 minute, by 41:00.".into()),
        ]
    );
}

#[test]
fn places_a_named_hero_in_the_one_unknown_slot() {
    use dota2coach::place_found_hero;
    use vision::heroes::Lineup;
    use vision::topbar::TopBar;

    let h = |s: &str| format!("npc_dota_hero_{s}");
    let mut lineup = Lineup {
        radiant: [h("lich"), h("luna"), h("ursa"), h("sniper"), h("lion")],
        dire: [
            h("shadow_shaman"),
            String::new(),
            h("drow_ranger"),
            h("kez"),
            h("zuus"),
        ],
    };
    let mut bar = TopBar::default();
    bar.dire[1].dead = true;
    assert_eq!(
        place_found_hero(&mut lineup, &h("juggernaut"), &bar),
        None,
        "grey while dead"
    );
    bar.dire[1].dead = false;
    assert_eq!(
        place_found_hero(&mut lineup, &h("lich"), &bar),
        None,
        "already placed"
    );
    assert_eq!(
        place_found_hero(&mut lineup, &h("juggernaut"), &bar),
        Some(6)
    );
    assert_eq!(lineup.dire[1], h("juggernaut"));
    assert_eq!(
        place_found_hero(&mut lineup, &h("axe"), &bar),
        None,
        "no unknown slot left"
    );
}

#[test]
fn times_roshan_from_his_skull_on_the_minimap() {
    use coach::GameSnapshot;
    use dota2coach::RoshanTimer;

    let mut timer = RoshanTimer::default();
    let mut at = |clock, shown| {
        let mut s = GameSnapshot {
            match_id: "1".into(),
            clock_time: clock,
            ..Default::default()
        };
        timer.observe(&mut s, shown);
        (s.roshan_dead, s.roshan_killed_at)
    };
    assert_eq!(at(2200, true), (false, 0));
    // A hero standing on the pit hides the skull for a few seconds.
    assert_eq!(at(2210, false), (false, 0));
    assert_eq!(at(2230, true), (false, 0));
    // Gone for a minute: killed when it went.
    assert_eq!(at(2268, false), (false, 0));
    assert_eq!(at(2300, false), (false, 0));
    assert_eq!(at(2328, false), (true, 2268));
    // Something red on the pit before he can be back changes nothing.
    assert_eq!(at(2450, true), (true, 2268));
    assert_eq!(at(2838, true), (false, 0), "back after 8 minutes");
}
