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
    assert!(said("Power and Bounty runes in 15 seconds."), "{spoken:#?}");
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
    assert!(said("能量符和赏金符15秒后刷新。"), "{spoken:#?}");
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
/// every shipped language should translate all of them.
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
            .any(|s| s == "You look like the offlaner, position 3. Coaching for that role."),
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
            .filter(|s| s.starts_with("You look like"))
            .count(),
        1
    );

    let spoken = replay_file(laning_recording("mid-zh", "radiant", (-1500, -1300)), "zh").await;
    assert!(
        spoken
            .iter()
            .any(|s| s == "看起来你在打二号位中单，接下来按这个位置给建议。"),
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
                    alive: true,
                    health_percent: if t % 100 < 50 { 100 } else { 20 },
                    gold: 2500,
                    deaths: 5,
                    since_death: t.rem_euclid(60),
                    gold_at_death: 2000,
                    buyback_cost: 3000,
                    last_hits: 1,
                    gpm: 200,
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
