//! The coaching pipeline shared by `run` (live) and `replay` (recorded):
//! GSI updates in, spoken advice out.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use coach::i18n::Arg;
use coach::{Advisor, Coordinator, GameSnapshot, Packs, RoleDetector, RuleEngine};
use gsi::Update;
use gsi::record::Recorder;
use tokio::sync::mpsc;
use tracing::{info, warn};
use voice::Speaker;

#[derive(Debug, Clone)]
pub struct CoachConfig {
    pub rules_dir: PathBuf,
    /// Role pack to load, for example `pos1`. `None` detects it during laning.
    pub role: Option<String>,
    /// Minimum game seconds between two spoken messages.
    pub min_gap_secs: i64,
    /// Language code of the message catalogs, for example `en` or `zh`.
    pub lang: String,
}

/// Consumes updates until the channel closes, speaking advice as it goes.
/// Returns every message that was spoken, in order.
pub async fn run_coach(
    mut rx: mpsc::Receiver<Update>,
    config: CoachConfig,
    speaker: &dyn Speaker,
    mut recorder: Option<Recorder>,
) -> anyhow::Result<Vec<String>> {
    let mut rules = RuleEngine::load(
        &config.rules_dir,
        Packs {
            role: config.role.clone(),
            hero: None,
        },
        &config.lang,
    )?;
    let mut coordinator = Coordinator::new(config.min_gap_secs);
    let mut detector = RoleDetector::default();
    let mut role = config.role.clone();
    let mut spoken = Vec::new();
    let mut say = |text: String| {
        speaker.say(&text);
        spoken.push(text);
    };

    while let Some(update) = rx.recv().await {
        if let Some(rec) = recorder.as_mut()
            && let Err(err) = rec.write(&update).await
        {
            warn!(%err, "couldn't record payload; recording stopped");
            recorder = None;
        }

        let Some(mut snapshot) = GameSnapshot::from_state(&update.state) else {
            continue;
        };
        if coordinator.observe(&snapshot) {
            info!(
                match_id = snapshot.match_id,
                hero = snapshot.hero,
                "new match"
            );
            detector.reset();
            role = config.role.clone();
        }
        // `--role` wins; otherwise work it out during laning.
        if config.role.is_none()
            && let Some(found) = detector.observe(&snapshot)
        {
            info!(role = found.role, lane = ?found.lane, "detected role");
            let args = BTreeMap::from([(
                "role".to_owned(),
                Arg::Keys(vec![format!("role.{}", found.role)]),
            )]);
            if let Some(text) = rules.catalog().render("role.detected", &args) {
                say(text);
            }
            role = Some(found.role);
        }
        snapshot.role = role.clone().unwrap_or_default();
        load_packs(&mut rules, &role, &snapshot);

        let candidates = rules.advise(&snapshot);
        if let Some(advice) = coordinator.select(&snapshot, candidates) {
            info!(
                clock = format_clock(snapshot.clock_time),
                rule = advice.rule_id,
                "{}",
                advice.message
            );
            say(advice.message);
        }
    }
    Ok(spoken)
}

/// Reloads rules when the role or hero pack changes: the hero once GSI reports
/// it, the role once it's detected.
fn load_packs(rules: &mut RuleEngine, role: &Option<String>, snapshot: &GameSnapshot) {
    let packs = Packs {
        role: role.clone(),
        hero: Some(snapshot.hero.clone()),
    };
    if *rules.packs() == packs {
        return;
    }
    if let Err(err) = rules.reload(packs) {
        warn!(%err, "couldn't load rules for this role or hero; keeping the previous set");
    }
}

/// `754` -> `12:34`, `-75` -> `-1:15`.
pub fn format_clock(secs: i64) -> String {
    let sign = if secs < 0 { "-" } else { "" };
    let secs = secs.abs();
    format!("{sign}{}:{:02}", secs / 60, secs % 60)
}

/// The rules folder: the one given, else `./rules`, else `rules` next to the
/// executable (how release builds are packaged).
pub fn resolve_rules_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(dir) = explicit {
        return dir.to_owned();
    }
    let local = PathBuf::from("rules");
    if local.is_dir() {
        return local;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("rules")))
        .filter(|dir| dir.is_dir())
        .unwrap_or(local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_game_clock() {
        assert_eq!(format_clock(754), "12:34");
        assert_eq!(format_clock(0), "0:00");
        assert_eq!(format_clock(-75), "-1:15");
    }
}
