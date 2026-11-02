//! `GameSnapshot`: the flat, rule-friendly view of the game the coach works on.
//!
//! This is the contract between inputs and advisors. Live GSI produces it today;
//! later, replays (masked to what a player could see) and a vision sidecar will
//! produce or extend the same struct, so rules, the LLM advisor and a trained
//! model all read one schema.

use gsi::GameState;
use serde::{Deserialize, Serialize};

const HERO_PREFIX: &str = "npc_dota_hero_";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameSnapshot {
    pub match_id: String,
    /// For example `DOTA_GAMERULES_STATE_GAME_IN_PROGRESS`.
    pub game_state: String,
    /// True from the pre-game countdown until the Ancient falls.
    pub in_game: bool,
    pub paused: bool,
    /// `radiant` or `dire` once the game is over, otherwise `none` or empty.
    pub win_team: String,
    /// The in-game clock in seconds; negative before the horn.
    pub clock_time: i64,
    pub daytime: bool,

    /// `radiant` or `dire`.
    pub team: String,
    /// The position being coached, for example `pos3`: from `--role`, or
    /// detected during laning. Empty until known.
    pub role: String,

    /// Short hero name, for example `axe` (without `npc_dota_hero_`).
    pub hero: String,
    /// Map position; about -8000 to 8000 on each axis, Radiant bottom left.
    pub xpos: i64,
    pub ypos: i64,
    pub level: i64,
    pub alive: bool,
    pub respawn_seconds: i64,
    pub health_percent: i64,
    pub mana_percent: i64,
    pub buyback_cost: i64,
    pub buyback_cooldown: i64,
    pub smoked: bool,
    /// Level of the hero's ultimate; 0 until learned.
    pub ult_level: i64,
    /// The ultimate is learned, off cooldown and castable (mana, not silenced).
    /// Hero rules use this rather than ability names, which change between
    /// patches.
    pub ult_ready: bool,

    pub gold: i64,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub last_hits: i64,
    pub denies: i64,
    pub gpm: i64,
    pub xpm: i64,
    /// Game seconds since the hero last died; -1 if it hasn't. Filled by
    /// [`History`](crate::History).
    pub since_death: i64,
    /// Gold the hero carried when it last died. Filled by `History`.
    pub gold_at_death: i64,

    /// Item names carried by the hero: inventory, backpack, TP slot and neutral
    /// slot, without empty slots. For example `["item_blink", "item_tpscroll"]`.
    pub items: Vec<String>,
    /// Item names sitting in the stash.
    pub stash: Vec<String>,

    /// The fields below come from the screen's top bar (`run --screen`).
    /// `screen` is false when it isn't being read, and the counts stay 0.
    pub screen: bool,
    pub enemies_dead: i64,
    /// Dead enemies with at least [`LONG_RESPAWN`] seconds left to respawn.
    pub enemies_dead_long: i64,
    /// Seconds until the first of those respawns: how long the team has to
    /// use the advantage.
    pub dead_window: i64,
    /// Teammates dead, not counting you.
    pub allies_dead: i64,

    /// The fields below come from the minimap (`run --screen`), counting
    /// only heroes you can see.
    pub enemies_seen: i64,
    /// World units to the nearest enemy seen; -1 when none is (0 while the
    /// minimap isn't being read).
    pub nearest_enemy: i64,
    /// Enemies seen within [`NEAR`] of you.
    pub enemies_near: i64,
    /// Teammates seen within [`NEAR`] of you, not counting you.
    pub allies_near: i64,
    /// Enemies seen on the minimap in the last few seconds that have since
    /// vanished without dying.
    pub enemies_missing: i64,

    /// Who is playing, read from the top bar's portraits (`run --screen`,
    /// after `fetch-heroes`): internal names like
    /// `npc_dota_hero_phantom_assassin`, left to right. Empty until the
    /// lineup has been read with confidence.
    pub ally_heroes: Vec<String>,
    pub enemy_heroes: Vec<String>,

    /// Items read off the item panel while you had a teammate or an enemy
    /// selected (`run --screen`, after `fetch-items`): every item seen on that
    /// side this match, like `item_black_king_bar`, and the game seconds
    /// since the last such read (-1 if none yet). Which hero carried each
    /// item isn't known yet.
    pub ally_items: Vec<String>,
    pub ally_items_age: i64,
    pub enemy_items: Vec<String>,
    pub enemy_items_age: i64,
}

/// World units within which a hero counts as near you: about a screen.
pub const NEAR: i64 = 1500;

/// A dead enemy with this many seconds left counts as out of the game for long
/// enough to take an objective.
pub const LONG_RESPAWN: i64 = 20;

impl GameSnapshot {
    /// Builds a snapshot, or `None` when the payload isn't from a match with a
    /// hero yet (main menu, hero selection).
    pub fn from_state(state: &GameState) -> Option<Self> {
        let map = state.map.as_ref()?;
        let hero = state.hero.as_ref()?;
        if hero.name.is_empty() {
            return None;
        }
        let player = state.player.clone().unwrap_or_default();

        let mut items = Vec::new();
        let mut stash = Vec::new();
        for (slot, item) in &state.items {
            if item.is_empty() {
                continue;
            }
            if slot.starts_with("stash") {
                stash.push(item.name.clone());
            } else {
                items.push(item.name.clone());
            }
        }

        let ult = state.abilities.values().find(|a| a.ultimate);

        Some(Self {
            match_id: map.matchid.clone(),
            game_state: map.game_state.clone(),
            in_game: matches!(
                map.game_state.as_str(),
                "DOTA_GAMERULES_STATE_PRE_GAME" | "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS"
            ),
            paused: map.paused,
            win_team: map.win_team.clone(),
            clock_time: map.clock_time,
            daytime: map.daytime,
            team: player.team_name.clone(),
            role: String::new(),
            hero: short_hero_name(&hero.name).to_owned(),
            xpos: hero.xpos,
            ypos: hero.ypos,
            level: hero.level,
            alive: hero.alive,
            respawn_seconds: hero.respawn_seconds,
            health_percent: hero.health_percent,
            mana_percent: hero.mana_percent,
            buyback_cost: hero.buyback_cost,
            buyback_cooldown: hero.buyback_cooldown,
            smoked: hero.smoked,
            ult_level: ult.map_or(0, |a| a.level),
            ult_ready: ult.is_some_and(|a| a.level > 0 && a.cooldown == 0 && a.can_cast),
            gold: player.gold,
            kills: player.kills,
            deaths: player.deaths,
            assists: player.assists,
            last_hits: player.last_hits,
            denies: player.denies,
            gpm: player.gpm,
            xpm: player.xpm,
            since_death: -1,
            gold_at_death: 0,
            items,
            stash,
            ..Default::default()
        })
    }

    pub fn has_item(&self, name: &str) -> bool {
        self.items.iter().any(|i| i == name)
    }
}

/// `npc_dota_hero_axe` -> `axe`.
pub fn short_hero_name(full: &str) -> &str {
    full.strip_prefix(HERO_PREFIX).unwrap_or(full)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn sample_state() -> GameState {
        serde_json::from_str(include_str!("../../gsi/tests/fixtures/player_state.json")).unwrap()
    }

    #[test]
    fn flattens_a_gsi_payload() {
        let snap = GameSnapshot::from_state(&sample_state()).unwrap();
        assert_eq!(snap.hero, "axe");
        assert_eq!(snap.clock_time, 754);
        assert!(snap.in_game);
        assert_eq!(snap.gold, 1312);
        assert_eq!(snap.team, "dire");
        // Culling Blade: level 2, castable, no cooldown.
        assert_eq!(snap.ult_level, 2);
        assert!(snap.ult_ready);
        assert_eq!((snap.xpos, snap.ypos), (-1820, 4200));
        assert!(snap.has_item("item_tpscroll"));
        assert!(snap.has_item("item_trusty_shovel"));
        assert!(!snap.items.iter().any(|i| i == "empty"));
        assert!(snap.stash.is_empty());
    }

    #[test]
    fn no_snapshot_without_a_hero() {
        let mut state = sample_state();
        state.hero = None;
        assert!(GameSnapshot::from_state(&state).is_none());
    }
}
