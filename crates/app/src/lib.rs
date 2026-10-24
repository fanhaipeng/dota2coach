//! The coaching pipeline shared by `run` (live) and `replay` (recorded):
//! GSI updates in, spoken advice out.

use std::path::{Path, PathBuf};

use coach::{Advisor, Coordinator, GameSnapshot, Packs, RuleEngine};
use gsi::Update;
use gsi::record::Recorder;
use tokio::sync::mpsc;
use tracing::{info, warn};
use voice::Speaker;

#[derive(Debug, Clone)]
pub struct CoachConfig {
    pub rules_dir: PathBuf,
    /// Role pack to load, for example `pos1`. Role inference comes later.
    pub role: Option<String>,
    /// Minimum game seconds between two spoken messages.
    pub min_gap_secs: i64,
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
    )?;
    let mut coordinator = Coordinator::new(config.min_gap_secs);
    let mut spoken = Vec::new();

    while let Some(update) = rx.recv().await {
        if let Some(rec) = recorder.as_mut()
            && let Err(err) = rec.write(&update).await
        {
            warn!(%err, "couldn't record payload; recording stopped");
            recorder = None;
        }

        let Some(snapshot) = GameSnapshot::from_state(&update.state) else {
            continue;
        };
        if coordinator.observe(&snapshot) {
            info!(
                match_id = snapshot.match_id,
                hero = snapshot.hero,
                "new match"
            );
        }
        load_hero_pack(&mut rules, &config, &snapshot);

        let candidates = rules.advise(&snapshot);
        if let Some(advice) = coordinator.select(&snapshot, candidates) {
            info!(
                clock = format_clock(snapshot.clock_time),
                rule = advice.rule_id,
                "{}",
                advice.message
            );
            speaker.say(&advice.message);
            spoken.push(advice.message);
        }
    }
    Ok(spoken)
}

/// Reloads rules with the hero pack once the hero is known or changes.
fn load_hero_pack(rules: &mut RuleEngine, config: &CoachConfig, snapshot: &GameSnapshot) {
    if rules.packs().hero.as_deref() == Some(snapshot.hero.as_str()) {
        return;
    }
    let packs = Packs {
        role: config.role.clone(),
        hero: Some(snapshot.hero.clone()),
    };
    if let Err(err) = rules.reload(packs) {
        warn!(%err, "couldn't load rules for this hero; keeping the previous set");
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
