//! Works out the player's position (pos1 to pos5) during the laning stage, so
//! nobody has to pass `--role`.
//!
//! GSI doesn't report the role picked in the queue, but it does report the
//! hero's map position and team. From 0:30 to 4:00 the detector notes which
//! lane the hero stands in:
//!
//! - mostly mid is pos2;
//! - mostly the team's safe lane is pos1, or pos5 if the hero looks like a
//!   support (carries wards, or has few last hits);
//! - mostly the offlane is pos3, or pos4 for a support;
//! - no lane most of the time (roaming, jungling) is pos4.
//!
//! It decides once per match, at 4:00 or as soon after as it has enough
//! samples, and gives up at 6:00.

use crate::snapshot::GameSnapshot;

/// When lane samples start counting: heroes have walked to their lanes.
const SAMPLE_FROM: i64 = 30;
/// When the detector decides.
const DECIDE_AT: i64 = 240;
/// When it stops trying, for example after a long death in lane.
const GIVE_UP_AT: i64 = 360;
/// Fewer samples than this is not enough to decide.
const MIN_SAMPLES: u32 = 20;
/// A lane needs at least this share of the samples to count as the hero's lane.
const MIN_SHARE: f64 = 0.5;
/// Cores have more last hits than this by 4:00; supports usually don't.
const SUPPORT_MAX_LAST_HITS: i64 = 10;

const SUPPORT_ITEMS: &[&str] = &[
    "item_ward_observer",
    "item_ward_sentry",
    "item_ward_dispenser",
    "item_smoke_of_deceit",
];

/// A lane from the player's team's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Safe,
    Mid,
    Off,
}

/// Where on the map a hero is, without regard to team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapLane {
    Top,
    Mid,
    Bottom,
}

/// The lane at a map position, or `None` in a base, the jungle or the river.
///
/// Map coordinates run from about -8000 to 8000 on both axes, with the Radiant
/// base bottom left and the Dire base top right. Top lane runs up the left
/// edge and along the top edge, bottom lane along the bottom edge and up the
/// right edge, and mid lane along the diagonal between the bases.
fn map_lane(x: i64, y: i64) -> Option<MapLane> {
    let in_base = (x < -5500 && y < -5000) || (x > 5500 && y > 5000);
    if in_base {
        None
    } else if (x - y).abs() < 2500 && x.abs() < 5500 {
        Some(MapLane::Mid)
    } else if x < -4500 || y > 4500 {
        Some(MapLane::Top)
    } else if x > 4500 || y < -4500 {
        Some(MapLane::Bottom)
    } else {
        None
    }
}

/// The lane at a map position for a player on `team` (`radiant` or `dire`).
pub fn lane_at(team: &str, x: i64, y: i64) -> Option<Lane> {
    let lane = map_lane(x, y)?;
    let radiant = match team {
        "radiant" => true,
        "dire" => false,
        _ => return None,
    };
    Some(match (lane, radiant) {
        (MapLane::Mid, _) => Lane::Mid,
        (MapLane::Bottom, true) | (MapLane::Top, false) => Lane::Safe,
        (MapLane::Top, true) | (MapLane::Bottom, false) => Lane::Off,
    })
}

/// What the detector concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// `pos1` to `pos5`, matching the folders in `rules/roles/`.
    pub role: String,
    /// The lane the hero spent most of the laning stage in, if any.
    pub lane: Option<Lane>,
}

#[derive(Debug, Clone, Default)]
pub struct RoleDetector {
    safe: u32,
    mid: u32,
    off: u32,
    elsewhere: u32,
    done: bool,
}

impl RoleDetector {
    /// Feeds one snapshot. Returns the detection once, when it is made; call
    /// [`reset`](Self::reset) when a new match starts.
    pub fn observe(&mut self, s: &GameSnapshot) -> Option<Detection> {
        if self.done || !s.in_game {
            return None;
        }
        if s.clock_time >= GIVE_UP_AT {
            self.done = true;
            return None;
        }
        // GSI reports 0,0 when it has no position.
        if s.clock_time >= SAMPLE_FROM && s.alive && (s.xpos, s.ypos) != (0, 0) {
            match lane_at(&s.team, s.xpos, s.ypos) {
                Some(Lane::Safe) => self.safe += 1,
                Some(Lane::Mid) => self.mid += 1,
                Some(Lane::Off) => self.off += 1,
                None => self.elsewhere += 1,
            }
        }
        if s.clock_time < DECIDE_AT {
            return None;
        }
        let total = self.safe + self.mid + self.off + self.elsewhere;
        if total < MIN_SAMPLES {
            return None;
        }
        self.done = true;

        let (count, lane) = [
            (self.safe, Lane::Safe),
            (self.mid, Lane::Mid),
            (self.off, Lane::Off),
        ]
        .into_iter()
        .max_by_key(|(count, _)| *count)?;
        let lane = (f64::from(count) / f64::from(total) >= MIN_SHARE).then_some(lane);
        let support =
            s.last_hits <= SUPPORT_MAX_LAST_HITS || SUPPORT_ITEMS.iter().any(|i| s.has_item(i));
        let role = match (lane, support) {
            (Some(Lane::Mid), _) => "pos2",
            (Some(Lane::Safe), false) => "pos1",
            (Some(Lane::Safe), true) => "pos5",
            (Some(Lane::Off), false) => "pos3",
            (Some(Lane::Off), true) | (None, _) => "pos4",
        };
        Some(Detection {
            role: role.to_owned(),
            lane,
        })
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_positions_to_lanes_per_team() {
        // Radiant's safe lane is bottom, Dire's is top.
        assert_eq!(lane_at("radiant", 4800, -6200), Some(Lane::Safe));
        assert_eq!(lane_at("dire", 4800, -6200), Some(Lane::Off));
        assert_eq!(lane_at("radiant", -6200, 1800), Some(Lane::Off));
        assert_eq!(lane_at("dire", -1800, 6000), Some(Lane::Safe));
        assert_eq!(lane_at("dire", 6200, -1600), Some(Lane::Off));
        assert_eq!(lane_at("radiant", -1500, -1300), Some(Lane::Mid));
        assert_eq!(lane_at("dire", 500, 300), Some(Lane::Mid));
        // Bases, jungle, unknown team.
        assert_eq!(lane_at("radiant", -7000, -6500), None);
        assert_eq!(lane_at("dire", 7000, 6400), None);
        assert_eq!(lane_at("radiant", 1500, -3000), None);
        assert_eq!(lane_at("", -1500, -1300), None);
    }

    /// Plays the laning stage at `pos`, one snapshot per second.
    fn laning(team: &str, pos: (i64, i64), last_hits: i64, items: &[&str]) -> Option<Detection> {
        let mut detector = RoleDetector::default();
        let mut found = None;
        for t in -60..=GIVE_UP_AT {
            let s = GameSnapshot {
                in_game: true,
                clock_time: t,
                alive: true,
                team: team.into(),
                xpos: pos.0,
                ypos: pos.1,
                last_hits,
                items: items.iter().map(|i| (*i).to_owned()).collect(),
                ..Default::default()
            };
            if let Some(d) = detector.observe(&s) {
                assert!(found.is_none(), "decided twice");
                assert_eq!(t, DECIDE_AT);
                found = Some(d);
            }
        }
        found
    }

    fn role(d: Option<Detection>) -> String {
        d.map(|d| d.role).unwrap_or_default()
    }

    #[test]
    fn infers_each_position() {
        assert_eq!(role(laning("radiant", (0, 0), 30, &[])), "", "no position");
        assert_eq!(role(laning("radiant", (-1500, -1300), 25, &[])), "pos2");
        assert_eq!(role(laning("radiant", (4800, -6200), 25, &[])), "pos1");
        assert_eq!(role(laning("radiant", (4800, -6200), 4, &[])), "pos5");
        assert_eq!(
            role(laning(
                "radiant",
                (4800, -6200),
                14,
                &["item_ward_observer"]
            )),
            "pos5"
        );
        assert_eq!(role(laning("dire", (4800, -6200), 20, &[])), "pos3");
        assert_eq!(role(laning("dire", (4800, -6200), 3, &[])), "pos4");
        assert_eq!(role(laning("dire", (1500, -3000), 3, &[])), "pos4");
    }

    #[test]
    fn decides_once_and_resets_for_a_new_match() {
        let mut detector = RoleDetector::default();
        let mut s = GameSnapshot {
            in_game: true,
            alive: true,
            team: "radiant".into(),
            xpos: -1500,
            ypos: -1300,
            last_hits: 30,
            ..Default::default()
        };
        let mut decided = 0;
        for t in 0..300 {
            s.clock_time = t;
            decided += usize::from(detector.observe(&s).is_some());
        }
        assert_eq!(decided, 1);
        detector.reset();
        s.clock_time = DECIDE_AT;
        assert!(detector.observe(&s).is_none(), "no samples yet after reset");
    }
}
