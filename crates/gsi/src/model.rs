//! Typed view of the JSON that Dota 2 posts through Game State Integration.
//!
//! Only the player-mode payload is modelled: while you are playing, GSI reports
//! your own hero and nothing about other players. Every field is optional or
//! defaulted, so a missing section (for example in the main menu) or a new field
//! added by a patch never makes a whole payload fail to parse.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One GSI payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameState {
    pub provider: Option<Provider>,
    pub map: Option<Map>,
    pub player: Option<Player>,
    pub hero: Option<Hero>,
    /// Keyed `ability0`, `ability1`, ...
    pub abilities: BTreeMap<String, Ability>,
    /// Keyed `slot0`..`slot8` (inventory and backpack), `stash0`..`stash5`,
    /// `teleport0` and `neutral0`.
    pub items: BTreeMap<String, Item>,
    pub auth: Option<Auth>,
}

impl GameState {
    /// The auth token Dota echoes back from the GSI config file, if any.
    pub fn token(&self) -> Option<&str> {
        self.auth.as_ref().map(|a| a.token.as_str())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Provider {
    pub name: String,
    pub appid: i64,
    pub version: i64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Map {
    pub name: String,
    pub matchid: String,
    /// Seconds since the map loaded.
    pub game_time: i64,
    /// The in-game clock: negative before the horn, 0 at the horn.
    pub clock_time: i64,
    pub daytime: bool,
    pub nightstalker_night: bool,
    /// For example `DOTA_GAMERULES_STATE_GAME_IN_PROGRESS`.
    pub game_state: String,
    pub paused: bool,
    pub win_team: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Player {
    pub steamid: String,
    pub name: String,
    pub activity: String,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub last_hits: i64,
    pub denies: i64,
    pub gold: i64,
    pub gold_reliable: i64,
    pub gold_unreliable: i64,
    pub gpm: i64,
    pub xpm: i64,
    pub team_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hero {
    pub xpos: i64,
    pub ypos: i64,
    pub id: i64,
    /// For example `npc_dota_hero_axe`.
    pub name: String,
    pub level: i64,
    pub alive: bool,
    pub respawn_seconds: i64,
    pub buyback_cost: i64,
    pub buyback_cooldown: i64,
    pub health: i64,
    pub max_health: i64,
    pub health_percent: i64,
    pub mana: i64,
    pub max_mana: i64,
    pub mana_percent: i64,
    pub silenced: bool,
    pub stunned: bool,
    pub disarmed: bool,
    pub magicimmune: bool,
    pub hexed: bool,
    pub muted: bool,
    pub smoked: bool,
    pub has_debuff: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ability {
    pub name: String,
    pub level: i64,
    pub can_cast: bool,
    pub passive: bool,
    pub ability_active: bool,
    pub cooldown: i64,
    pub ultimate: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    /// `empty` for an empty slot.
    pub name: String,
    pub purchaser: Option<i64>,
    pub charges: Option<i64>,
    pub can_cast: Option<bool>,
    pub cooldown: Option<i64>,
}

impl Item {
    pub fn is_empty(&self) -> bool {
        self.name.is_empty() || self.name == "empty"
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Auth {
    pub token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/player_state.json");

    #[test]
    fn parses_a_player_mode_payload() {
        let state: GameState = serde_json::from_str(SAMPLE).unwrap();
        let map = state.map.as_ref().unwrap();
        assert_eq!(map.clock_time, 754);
        assert_eq!(map.game_state, "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS");

        let hero = state.hero.as_ref().unwrap();
        assert_eq!(hero.name, "npc_dota_hero_axe");
        assert_eq!(hero.level, 11);
        assert!(hero.alive);

        assert_eq!(state.player.as_ref().unwrap().gold, 1312);
        assert_eq!(state.items["teleport0"].name, "item_tpscroll");
        assert!(state.items["slot5"].is_empty());
        assert_eq!(state.abilities["ability3"].name, "axe_culling_blade");
        assert_eq!(state.token(), Some("dota2coach-local"));
    }

    #[test]
    fn tolerates_an_empty_menu_payload() {
        let state: GameState =
            serde_json::from_str(r#"{"provider":{"name":"Dota 2","appid":570}}"#).unwrap();
        assert!(state.map.is_none());
        assert!(state.hero.is_none());
    }
}
