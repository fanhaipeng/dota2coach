//! The coaching pipeline shared by `run` (live), `replay` and `review`
//! (recorded): GSI updates in, spoken advice out.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use coach::i18n::Arg;
use coach::{
    Advisor, Catalog, Coordinator, GameSnapshot, History, Packs, Report, RoleDetector, RuleEngine,
    Said,
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
/// every snapshot after the role and history are filled in, and can add to it
/// (the screen fields) before the rules run.
pub async fn run_coach(
    mut rx: mpsc::Receiver<Update>,
    config: CoachConfig,
    speaker: &dyn Speaker,
    mut recording: Recording,
    mut observe: Option<&mut dyn FnMut(&mut GameSnapshot)>,
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
            observe(&mut snapshot);
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
    let mut collect = |s: &mut GameSnapshot| snapshots.push(s.clone());
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

/// An ally with this many seconds or fewer left to respawn doesn't count as
/// dead.
const ALLY_BACK_SOON: i64 = 5;

/// Fills the snapshot's screen fields from a top-bar reading. Does nothing
/// until GSI has said which team the player is on.
pub fn apply_top_bar(s: &mut GameSnapshot, bar: &vision::topbar::TopBar) {
    let (allies, enemies) = match s.team.as_str() {
        "radiant" => (&bar.radiant, &bar.dire),
        "dire" => (&bar.dire, &bar.radiant),
        _ => return,
    };
    let long: Vec<i64> = enemies
        .iter()
        .filter(|slot| slot.dead)
        .filter_map(|slot| slot.respawn.map(i64::from))
        .filter(|secs| *secs >= coach::snapshot::LONG_RESPAWN)
        .collect();
    let dead =
        |side: &[vision::topbar::Slot; 5]| side.iter().filter(|slot| slot.dead).count() as i64;
    s.screen = true;
    s.enemies_dead = dead(enemies);
    s.enemies_dead_long = long.len() as i64;
    s.dead_window = long.iter().copied().min().unwrap_or(0);
    // Allies about to respawn are as good as alive.
    let out = |slot: &&vision::topbar::Slot| {
        slot.dead
            && slot
                .respawn
                .is_none_or(|secs| i64::from(secs) > ALLY_BACK_SOON)
    };
    // The top bar shows you too; GSI knows whether you're dead (its timer is
    // one less than the screen's).
    let you = !s.alive && s.respawn_seconds + 1 > ALLY_BACK_SOON;
    s.allies_dead = (allies.iter().filter(out).count() as i64 - i64::from(you)).max(0);
}

/// Where `fetch-heroes` saves Valve's hero portraits.
pub const DEFAULT_HEROES_DIR: &str = "assets/heroes";
/// Where `fetch-items` saves Valve's item icons.
pub const DEFAULT_ITEMS_DIR: &str = "assets/items";
/// Public lists of heroes and items with their image paths.
const DOTACONSTANTS: &str = "https://raw.githubusercontent.com/odota/dotaconstants/master/build";
const STEAM_CDN: &str = "https://cdn.cloudflare.steamstatic.com";

/// Downloads every hero's portrait into `dir` as `<short name>.png`, for
/// [`vision::heroes::References::load`]. Returns how many were saved.
pub async fn fetch_heroes(dir: &Path) -> anyhow::Result<usize> {
    #[derive(serde::Deserialize)]
    struct Hero {
        name: String,
        img: String,
    }
    let heroes: BTreeMap<String, Hero> = fetch_json("heroes.json").await?;
    let images = heroes.into_values().filter_map(|hero| {
        let short = hero.name.strip_prefix("npc_dota_hero_")?.to_owned();
        Some((short, hero.img))
    });
    fetch_images(dir, images).await
}

/// Downloads every item's icon into `dir` as `<short name>.png`, for
/// example `blink.png` for `item_blink`. Returns how many were saved.
pub async fn fetch_items(dir: &Path) -> anyhow::Result<usize> {
    #[derive(serde::Deserialize)]
    struct Item {
        img: Option<String>,
    }
    let items: BTreeMap<String, Item> = fetch_json("items.json").await?;
    let images = items
        .into_iter()
        .filter_map(|(short, item)| Some((short, item.img?)));
    fetch_images(dir, images).await
}

async fn fetch_json<T: serde::de::DeserializeOwned>(file: &str) -> anyhow::Result<T> {
    Ok(reqwest::get(format!("{DOTACONSTANTS}/{file}"))
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Saves each (short name, CDN path) image into `dir` as `<short name>.png`.
async fn fetch_images(
    dir: &Path,
    images: impl Iterator<Item = (String, String)>,
) -> anyhow::Result<usize> {
    let client = reqwest::Client::new();
    std::fs::create_dir_all(dir)?;
    let mut saved = 0;
    for (short, img) in images {
        // Paths look like `/apps/dota2/images/dota_react/heroes/axe.png?`.
        let path = img.split('?').next().unwrap_or_default();
        let response = client.get(format!("{STEAM_CDN}{path}")).send().await?;
        if !response.status().is_success() {
            warn!(short, status = %response.status(), "skipping an image the CDN doesn't have");
            continue;
        }
        let bytes = response.bytes().await?;
        // Keep only real images.
        if image::load_from_memory(&bytes).is_err() {
            warn!(short, "skipping a file that isn't an image");
            continue;
        }
        std::fs::write(dir.join(format!("{short}.png")), &bytes)?;
        saved += 1;
    }
    Ok(saved)
}
/// The hero portraits folder: the one given if it exists, else
/// [`DEFAULT_HEROES_DIR`] here or next to the executable. `None` until
/// `fetch-heroes` has been run.
pub fn resolve_heroes_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    resolve_assets(explicit, DEFAULT_HEROES_DIR)
}

/// The item icons folder, like [`resolve_heroes_dir`]; `None` until
/// `fetch-items` has been run.
pub fn resolve_items_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    resolve_assets(explicit, DEFAULT_ITEMS_DIR)
}

fn resolve_assets(explicit: Option<&Path>, default: &str) -> Option<PathBuf> {
    let candidates = [
        explicit.map(Path::to_owned),
        Some(PathBuf::from(default)),
        std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join(default))),
    ];
    candidates.into_iter().flatten().find(|dir| dir.is_dir())
}
/// Fills the snapshot's hero names from a lineup read off the top bar.
pub fn apply_lineup(s: &mut GameSnapshot, lineup: &vision::heroes::Lineup) {
    let (allies, enemies) = match s.team.as_str() {
        "radiant" => (&lineup.radiant, &lineup.dire),
        "dire" => (&lineup.dire, &lineup.radiant),
        _ => return,
    };
    let known = |heroes: &[String]| heroes.iter().filter(|h| !h.is_empty()).cloned().collect();
    s.ally_heroes = known(allies);
    s.enemy_heroes = known(enemies);
}

/// One reading of the item panel: whose side the hero is on, who it is if
/// its name was recognised, and its six inventory items.
#[derive(Debug, Clone, Default)]
pub struct PanelRead {
    pub side: Option<vision::items::Side>,
    /// Internal hero name, for example `npc_dota_hero_axe`.
    pub hero: Option<String>,
    pub items: [Option<String>; 6],
}

/// Remembers the items read off the panel while you have another hero
/// selected, per side and per hero, for one match.
#[derive(Debug, Default)]
pub struct PanelItems {
    match_id: String,
    /// Items seen and the clock of the last read: allies, then enemies.
    seen: [(Vec<String>, Option<i64>); 2],
    /// Each named hero's items at its last read, and when.
    heroes: BTreeMap<String, (Vec<String>, i64)>,
    /// Heroes the lineup couldn't tell (an alternative portrait) but whose
    /// name was read in the panel: allies, then enemies.
    found: [Vec<String>; 2],
}

impl PanelItems {
    /// Records a panel reading and fills the snapshot's panel item fields.
    /// Call after [`apply_lineup`].
    pub fn observe(&mut self, s: &mut GameSnapshot, read: Option<&PanelRead>) {
        use coach::snapshot::STALE_ITEMS;

        if self.match_id != s.match_id {
            *self = Self {
                match_id: s.match_id.clone(),
                ..Default::default()
            };
        }
        if let Some(read) = read
            && let Some(side) = read.side
        {
            let items: Vec<&String> = read.items.iter().flatten().collect();
            // The panel shows you if it has your name, or, unnamed, unless
            // its items differ from yours (your own bar can look red while
            // you're dead).
            let yours = items.iter().filter(|i| s.items.contains(i)).count();
            // A name read wrong (a hero label next to the portrait) still
            // shows your own items: three or more, all yours, is you.
            let all_yours = items.len() >= 3 && yours == items.len();
            let is_you = match &read.hero {
                Some(hero) => {
                    hero.strip_prefix("npc_dota_hero_") == Some(s.hero.as_str()) || all_yours
                }
                None => items.is_empty() || yours * 2 >= items.len(),
            };
            // An ally's bar with an enemy's name is your own panel with a
            // hero label read as its name (allies misread as enemies happen,
            // this the other way round doesn't).
            let misnamed = side == vision::items::Side::Ally
                && read
                    .hero
                    .as_ref()
                    .is_some_and(|h| s.enemy_heroes.contains(h));
            // A panel with no items tells nothing (and may be a unit, not a
            // hero).
            if !is_you && !misnamed && !items.is_empty() {
                let (seen, at) = &mut self.seen[usize::from(side == vision::items::Side::Enemy)];
                for &item in &items {
                    if !seen.contains(item) {
                        seen.push(item.clone());
                    }
                }
                *at = Some(s.clock_time);
                if let Some(hero) = &read.hero {
                    let items = items.into_iter().cloned().collect();
                    self.heroes.insert(hero.clone(), (items, s.clock_time));
                    let side = usize::from(side == vision::items::Side::Enemy);
                    let known = s.ally_heroes.contains(hero) || s.enemy_heroes.contains(hero);
                    if !known && !self.found[side].contains(hero) {
                        self.found[side].push(hero.clone());
                    }
                }
            }
        }
        for (heroes, found) in [
            (&mut s.ally_heroes, &self.found[0]),
            (&mut s.enemy_heroes, &self.found[1]),
        ] {
            for hero in found {
                if !heroes.contains(hero) && heroes.len() < 5 {
                    heroes.push(hero.clone());
                }
            }
        }
        let age = |at: Option<i64>| at.map_or(-1, |at| (s.clock_time - at).max(0));
        s.ally_items = self.seen[0].0.clone();
        s.ally_items_age = age(self.seen[0].1);
        s.enemy_items = self.seen[1].0.clone();
        s.enemy_items_age = age(self.seen[1].1);
        s.hero_items = self
            .heroes
            .iter()
            .map(|(hero, (items, _))| (hero.clone(), items.clone()))
            .collect();
        s.hero_items_age = self
            .heroes
            .iter()
            .map(|(hero, (_, at))| (hero.clone(), age(Some(*at))))
            .collect();
        // Never read counts from the start of the game.
        let stalest = s
            .enemy_heroes
            .iter()
            .map(|hero| {
                let at = self.heroes.get(hero).map_or(0, |h| h.1);
                (s.clock_time - at, hero)
            })
            .max_by_key(|(age, _)| *age);
        s.stale_enemy = match stalest {
            Some((age, hero)) if age >= STALE_ITEMS => hero.clone(),
            _ => String::new(),
        };
    }
}
/// The heroes the selected hero's name is matched against: the lineup, and
/// while a slot is unknown (an alternative portrait the lineup couldn't place)
/// every hero not in it, matched more strictly.
pub fn panel_candidates(
    lineup: Option<&vision::heroes::Lineup>,
    all: &[String],
) -> (Vec<String>, Vec<String>) {
    let Some(lineup) = lineup else {
        return (Vec::new(), Vec::new());
    };
    let placed: Vec<&String> = lineup.radiant.iter().chain(&lineup.dire).collect();
    let candidates: Vec<String> = placed
        .iter()
        .filter(|h| !h.is_empty())
        .map(|h| h.to_string())
        .collect();
    let outsiders = if placed.iter().any(|h| h.is_empty()) {
        all.iter()
            .filter(|h| !placed.contains(h))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    (candidates, outsiders)
}

/// Puts a hero named in the panel into the lineup slot the top bar couldn't
/// tell, when there's exactly one such slot and its hero is alive (so its
/// portrait is in colour). Returns the slot (0 to 4 Radiant, 5 to 9 Dire).
pub fn place_found_hero(
    lineup: &mut vision::heroes::Lineup,
    hero: &str,
    bar: &vision::topbar::TopBar,
) -> Option<usize> {
    let slots: Vec<&String> = lineup.radiant.iter().chain(&lineup.dire).collect();
    if slots.iter().any(|h| *h == hero) {
        return None;
    }
    let [slot] = slots
        .iter()
        .enumerate()
        .filter(|(_, h)| h.is_empty())
        .map(|(i, _)| i)
        .collect::<Vec<_>>()[..]
    else {
        return None;
    };
    let dead = bar.radiant.iter().chain(&bar.dire).nth(slot)?.dead;
    if dead {
        return None;
    }
    let side = if slot < 5 {
        &mut lineup.radiant
    } else {
        &mut lineup.dire
    };
    side[slot % 5] = hero.to_owned();
    Some(slot)
}

/// Game seconds Roshan's skull must be missing from the minimap to count him
/// dead: heroes standing on it hide it for a few seconds.
const ROSHAN_GONE: i64 = 60;
/// Roshan respawns 8 to 11 minutes after he dies.
const ROSHAN_BACK: std::ops::RangeInclusive<i64> = 480..=660;

/// Works out when Roshan died from his skull on the minimap, for one match.
#[derive(Debug, Default)]
pub struct RoshanTimer {
    match_id: String,
    /// Game clock since when the skull has been missing.
    missing_since: Option<i64>,
    /// Game clock when he died, while he's dead.
    killed_at: Option<i64>,
}

impl RoshanTimer {
    /// Records whether the skull shows now and fills the snapshot's Roshan
    /// fields.
    pub fn observe(&mut self, s: &mut GameSnapshot, shown: bool) {
        if self.match_id != s.match_id {
            *self = Self {
                match_id: s.match_id.clone(),
                ..Default::default()
            };
        }
        let clock = s.clock_time;
        if shown {
            self.missing_since = None;
            // Back already: the skull shows again once he has respawned.
            if self
                .killed_at
                .is_some_and(|at| clock - at >= *ROSHAN_BACK.start())
            {
                self.killed_at = None;
            }
        } else {
            let since = *self.missing_since.get_or_insert(clock);
            if self.killed_at.is_none() && clock - since >= ROSHAN_GONE {
                self.killed_at = Some(since);
            }
        }
        // Surely back by now, even if nobody has looked.
        if self
            .killed_at
            .is_some_and(|at| clock - at > *ROSHAN_BACK.end() + ROSHAN_GONE)
        {
            self.killed_at = None;
            self.missing_since = None;
        }
        s.roshan_dead = self.killed_at.is_some();
        s.roshan_killed_at = self.killed_at.unwrap_or(0);
    }
}

/// Hero names in every language of the rules folder (its `[hero]` tables),
/// for recognising the name written above the selected hero's face whatever
/// language Dota is set to.
pub struct HeroNames {
    catalogs: Vec<Catalog>,
}

/// A name must match at least this well (0 to 1), and better than the next
/// hero by this much, to count.
const MIN_NAME_MATCH: f64 = 0.5;
/// A hero outside the lineup (filling a slot it couldn't tell) must match
/// this well: any of a hundred names can half-match a misread.
const MIN_OUTSIDER_MATCH: f64 = 0.8;
const MIN_NAME_LEAD: f64 = 0.15;

impl HeroNames {
    pub fn load(rules_dir: &Path) -> anyhow::Result<Self> {
        let catalogs = coach::i18n::available(rules_dir)
            .iter()
            .map(|lang| Catalog::load(lang, &[rules_dir.to_owned()]))
            .collect::<anyhow::Result<_>>()?;
        Ok(Self { catalogs })
    }

    /// A hero's name in each language, and its nicknames (`[hero_alias]`,
    /// several separated by |), from its internal name.
    fn names(&self, hero: &str) -> Vec<String> {
        let short = hero.strip_prefix("npc_dota_hero_").unwrap_or(hero);
        let mut names = Vec::new();
        for catalog in &self.catalogs {
            for table in ["hero", "hero_alias"] {
                if let Some(text) = catalog.render(&format!("{table}.{short}"), &BTreeMap::new()) {
                    names.extend(text.split('|').map(str::to_owned));
                }
            }
        }
        names
    }

    /// Which of `candidates` (internal names) the recognised `texts` name,
    /// if one stands out.
    pub fn identify<'a>(&self, texts: &[String], candidates: &'a [String]) -> Option<&'a str> {
        self.identify_at(texts, candidates, MIN_NAME_MATCH)
    }

    /// Like [`identify`](Self::identify), first among the `lineup`, then
    /// among `outsiders` with a stricter match.
    pub fn identify_in<'a>(
        &self,
        texts: &[String],
        lineup: &'a [String],
        outsiders: &'a [String],
    ) -> Option<&'a str> {
        self.identify_at(texts, lineup, MIN_NAME_MATCH)
            .or_else(|| self.identify_at(texts, outsiders, MIN_OUTSIDER_MATCH))
    }

    fn identify_at<'a>(
        &self,
        texts: &[String],
        candidates: &'a [String],
        min: f64,
    ) -> Option<&'a str> {
        let mut scores: Vec<(f64, &str)> = candidates
            .iter()
            .map(|hero| {
                let best = self
                    .names(hero)
                    .iter()
                    .flat_map(|name| texts.iter().map(move |text| likeness(text, name)))
                    .fold(0.0, f64::max);
                (best, hero.as_str())
            })
            .collect();
        scores.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (best, hero) = *scores.first()?;
        let second = scores.get(1).map_or(0.0, |s| s.0);
        (best >= min && best - second >= MIN_NAME_LEAD).then_some(hero)
    }
}

/// Recognises the selected hero's name in panel crops, reading the text
/// again only when the letters change (the world behind them moves all the
/// time, the letters don't).
pub struct PanelNamer {
    names: HeroNames,
    languages: Vec<String>,
    /// The letters last read, and the hero they named.
    last: Option<([image::RgbImage; 2], Option<String>)>,
}

/// Letter pictures differing in less than this share of pixels are the same
/// name.
const SAME_LETTERS: f64 = 0.02;

impl PanelNamer {
    pub fn new(names: HeroNames) -> Self {
        Self {
            names,
            languages: vision::ocr::languages(),
            last: None,
        }
    }

    /// The selected hero, out of the lineup or, more strictly, `outsiders`.
    pub fn name(
        &mut self,
        panel: &image::RgbImage,
        lineup: &[String],
        outsiders: &[String],
    ) -> Option<String> {
        let pictures = vision::items::name_pictures(panel);
        if let Some((letters, hero)) = &self.last
            && differing(&letters[0], &pictures[1]) < SAME_LETTERS
            && differing(&letters[1], &pictures[3]) < SAME_LETTERS
        {
            return hero.clone();
        }
        let mut texts = Vec::new();
        for lang in &self.languages {
            for picture in &pictures {
                if let Ok(text) = vision::ocr::read_text(picture, lang)
                    && !text.trim().is_empty()
                {
                    texts.push(text);
                }
            }
        }
        let hero = self
            .names
            .identify_in(&texts, lineup, outsiders)
            .map(str::to_owned);
        let [_, small, _, large, ..] = pictures;
        self.last = Some(([small, large], hero.clone()));
        hero
    }
}

/// The share of pixels that differ between two same-sized pictures.
fn differing(a: &image::RgbImage, b: &image::RgbImage) -> f64 {
    if a.dimensions() != b.dimensions() {
        return 1.0;
    }
    let differ = a.pixels().zip(b.pixels()).filter(|(p, q)| p != q).count();
    differ as f64 / (a.width() * a.height()).max(1) as f64
}

/// How alike two names are, from 0 to 1: the share of letters they have in
/// common, ignoring case, spaces and punctuation. Forgiving of the odd
/// misread letter, and works for any script.
fn likeness(a: &str, b: &str) -> f64 {
    let letters = |s: &str| -> Vec<char> {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let (a, mut b) = (letters(a), letters(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let total = (a.len() + b.len()) as f64;
    let mut common = 0;
    for c in a {
        if let Some(i) = b.iter().position(|&d| d == c) {
            b.swap_remove(i);
            common += 1;
        }
    }
    2.0 * common as f64 / total
}

/// An ally icon this close to where GSI puts you is you.
const YOU_RADIUS: f64 = 500.0;
/// How long a seen enemy is remembered when counting the missing.
const MISSING_WINDOW: i64 = 20;

/// Fills the snapshot's minimap fields from a minimap reading, around your
/// position from GSI.
pub fn apply_minimap(s: &mut GameSnapshot, map: &vision::minimap::Minimap) {
    use coach::snapshot::NEAR;

    let you = vision::minimap::Point {
        x: s.xpos,
        y: s.ypos,
    };
    let enemies: Vec<f64> = map.enemies.iter().map(|e| e.distance(you)).collect();
    let mut allies: Vec<f64> = map.allies.iter().map(|a| a.distance(you)).collect();
    // Your own icon is the ally closest to you, if it's where GSI says.
    allies.sort_by(f64::total_cmp);
    if s.alive && allies.first().is_some_and(|&d| d < YOU_RADIUS) {
        allies.remove(0);
    }
    s.screen = true;
    s.enemies_seen = enemies.len() as i64;
    s.nearest_enemy = enemies
        .iter()
        .copied()
        .min_by(f64::total_cmp)
        .map_or(-1, |d| d.round() as i64);
    s.enemies_near = enemies.iter().filter(|&&d| d <= NEAR as f64).count() as i64;
    s.allies_near = allies.iter().filter(|&&d| d <= NEAR as f64).count() as i64;
    s.allies_close = allies
        .iter()
        .filter(|&&d| d <= coach::snapshot::CLOSE as f64)
        .count() as i64;
}

/// Counts enemies that dropped off the minimap without dying: the most seen
/// alive in the last [`MISSING_WINDOW`] seconds, less those seen or dead now.
#[derive(Debug, Default)]
pub struct MissingEnemies {
    /// (clock, enemies seen, enemies dead) for recent readings.
    recent: std::collections::VecDeque<(i64, i64, i64)>,
}

impl MissingEnemies {
    /// Sets `enemies_missing`; call after [`apply_top_bar`] and
    /// [`apply_minimap`] for the same moment.
    pub fn observe(&mut self, s: &mut GameSnapshot) {
        while self
            .recent
            .front()
            .is_some_and(|&(at, ..)| at > s.clock_time || s.clock_time - at > MISSING_WINDOW)
        {
            self.recent.pop_front();
        }
        self.recent
            .push_back((s.clock_time, s.enemies_seen, s.enemies_dead));
        // An enemy seen then and dead now isn't missing.
        let accounted_now = s.enemies_seen + s.enemies_dead;
        let peak = self
            .recent
            .iter()
            .map(|&(_, seen, dead)| seen + dead.min(s.enemies_dead))
            .max()
            .unwrap_or(0);
        s.enemies_missing = (peak - accounted_now).max(0);
    }
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
