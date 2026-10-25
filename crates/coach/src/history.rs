//! Facts that need more than one snapshot, added to each snapshot so rules
//! can stay simple, one-moment checks.
//!
//! For example "you died carrying 1800 gold": by the time GSI reports you
//! dead, the gold you lost is already gone, so the history remembers what you
//! had while alive.

use crate::snapshot::GameSnapshot;

#[derive(Debug, Clone, Default)]
pub struct History {
    was_alive: bool,
    last_gold_alive: i64,
    death_clock: Option<i64>,
    gold_at_death: i64,
}

impl History {
    /// Fills `since_death` and `gold_at_death` on `s` from earlier snapshots.
    pub fn observe(&mut self, s: &mut GameSnapshot) {
        if self.was_alive && !s.alive {
            self.death_clock = Some(s.clock_time);
            self.gold_at_death = self.last_gold_alive;
        }
        if s.alive {
            self.last_gold_alive = s.gold;
        }
        self.was_alive = s.alive;
        s.since_death = self.death_clock.map_or(-1, |t| s.clock_time - t);
        s.gold_at_death = self.gold_at_death;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(clock: i64, alive: bool, gold: i64) -> GameSnapshot {
        GameSnapshot {
            clock_time: clock,
            alive,
            gold,
            ..Default::default()
        }
    }

    #[test]
    fn remembers_the_gold_carried_into_a_death() {
        let mut h = History::default();
        let mut s = at(100, true, 1800);
        h.observe(&mut s);
        assert_eq!(s.since_death, -1);

        // Dead: GSI already shows the gold after the loss.
        let mut s = at(102, false, 1100);
        h.observe(&mut s);
        assert_eq!((s.since_death, s.gold_at_death), (0, 1800));
        let mut s = at(110, false, 1100);
        h.observe(&mut s);
        assert_eq!(s.since_death, 8);

        let mut s = at(140, true, 1200);
        h.observe(&mut s);
        assert_eq!((s.since_death, s.gold_at_death), (38, 1800));
    }
}
