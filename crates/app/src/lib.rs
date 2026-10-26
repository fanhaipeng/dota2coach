//! The coaching pipeline shared by `run` (live), `replay` and `review`
//! (recorded): GSI updates in, spoken advice out.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use coach::i18n::Arg;
use coach::{
    Advisor, Coordinator, GameSnapshot, History, Packs, Report, RoleDetector, RuleEngine, Said,
};
use gsi::Update;
use gsi::record::{self, Recorder};
use tokio::sync::mpsc;
use tracing::{info, warn};
use voice::Speaker;

pub mod steam;
pub mod writeup;

/// Where payloads are saved while coaching.
pub enum Recording {
    Off,
    /// Everything into one file (`run --record FILE`).
    File(Recorder),
    /// One file per match in this folder, named by date and match id, so
    /// every game can be reviewed afterwards.
    PerMatch {
        dir: PathBuf,
        current: Option<(String, Recorder)>,
    },
}

impl Recording {
    pub fn per_match(dir: impl Into<PathBuf>) -> Self {
        Recording::PerMatch {
            dir: dir.into(),
            current: None,
        }
    }

    async fn write(&mut self, update: &Update) -> anyhow::Result<()> {
        match self {
            Recording::Off => Ok(()),
            Recording::File(rec) => rec.write(update).await,
            Recording::PerMatch { dir, current } => {
                // Menus and hero selection before a match id aren't kept.
                let Some(match_id) = update
                    .state
                    .map
                    .as_ref()
                    .map(|m| m.matchid.as_str())
                    .filter(|id| !id.is_empty() && *id != "0")
                else {
                    return Ok(());
                };
                if current.as_ref().is_none_or(|(id, _)| id != match_id) {
                    std::fs::create_dir_all(&*dir)?;
                    let path = dir.join(format!("{}-{match_id}.jsonl", file_stamp()));
                    info!(path = %path.display(), "recording this match");
                    *current = Some((match_id.to_owned(), Recorder::create(&path).await?));
                }
                let (_, rec) = current.as_mut().expect("set above");
                rec.write(update).await
            }
        }
    }
}

/// `2026-10-25_2131` in UTC, for recording file names that sort by time.
fn file_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let day_secs = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}{:02}",
        day_secs / 3600,
        day_secs % 3600 / 60
    )
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

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
/// Returns every message that was spoken, in order. `observe`, if given, sees
/// every snapshot after the role and history are filled in.
pub async fn run_coach(
    mut rx: mpsc::Receiver<Update>,
    config: CoachConfig,
    speaker: &dyn Speaker,
    mut recording: Recording,
    mut observe: Option<&mut dyn FnMut(&GameSnapshot)>,
) -> anyhow::Result<Vec<Said>> {
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
    let mut history = History::default();
    let mut role = config.role.clone();
    let mut spoken = Vec::new();
    let mut say = |clock: i64, rule_id: &str, text: String| {
        speaker.say(&text);
        spoken.push(Said {
            clock,
            rule_id: rule_id.to_owned(),
            text,
        });
    };

    while let Some(update) = rx.recv().await {
        if let Err(err) = recording.write(&update).await {
            warn!(%err, "couldn't record payload; recording stopped");
            recording = Recording::Off;
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
            history.reset();
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
                say(snapshot.clock_time, "role.detected", text);
            }
            role = Some(found.role);
        }
        history.observe(&mut snapshot);
        snapshot.role = role.clone().unwrap_or_default();
        load_packs(&mut rules, &role, &snapshot);
        if let Some(observe) = observe.as_mut() {
            observe(&snapshot);
        }

        let candidates = rules.advise(&snapshot);
        if let Some(advice) = coordinator.select(&snapshot, candidates) {
            info!(
                clock = format_clock(snapshot.clock_time),
                rule = advice.rule_id,
                "{}",
                advice.message
            );
            say(snapshot.clock_time, &advice.rule_id, advice.message);
        }
    }
    Ok(spoken)
}

/// Reviews a recorded game: replays it through the coach silently (detecting
/// the role as in a live game) and works out the facts. `None` if the
/// recording has no match in it.
pub async fn review_file(path: &Path, config: CoachConfig) -> anyhow::Result<Option<Report>> {
    struct Silent;
    impl Speaker for Silent {
        fn say(&self, _: &str) {}
    }

    let (tx, rx) = mpsc::channel(1024);
    let file = path.to_owned();
    let reader = tokio::spawn(async move { record::replay(&file, 0.0, tx).await });
    let mut snapshots = Vec::new();
    let mut collect = |s: &GameSnapshot| snapshots.push(s.clone());
    let said = run_coach(rx, config, &Silent, Recording::Off, Some(&mut collect)).await?;
    reader.await??;
    Ok(coach::review::analyze(&snapshots, &said))
}

/// The newest `.jsonl` recording in `dir`.
pub fn latest_recording(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .max_by_key(|p| {
            p.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        })
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

pub use coach::review::format_clock;

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
    fn stamps_files_with_the_date() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_386), (2025, 10, 25));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(file_stamp().len(), "2026-10-25_2131".len());
    }

    #[test]
    fn formats_the_game_clock() {
        assert_eq!(format_clock(754), "12:34");
        assert_eq!(format_clock(0), "0:00");
        assert_eq!(format_clock(-75), "-1:15");
    }
}
