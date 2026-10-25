//! End to end: replay the sample recording through the real rules and check
//! what the coach says.

use std::path::PathBuf;
use std::sync::Mutex;

use dota2coach::{CoachConfig, run_coach};
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
    let spoken = run_coach(rx, config, &speaker, None).await.unwrap();
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
    for pack in ["", "roles/pos1", "roles/pos3", "heroes/axe"] {
        let dir = root.join(pack).join("lang");
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
