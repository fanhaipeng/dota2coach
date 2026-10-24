//! Decides what actually gets said, and when.
//!
//! Advisors can fire many times per second. The coordinator turns that into a
//! calm stream of speech:
//!
//! - each rule respects its own cooldown, measured in game-clock seconds, so
//!   replaying a recording faster behaves the same as playing live;
//! - at most one message is spoken per `min_gap_secs`, and the most urgent
//!   candidate wins;
//! - nothing is said while paused or outside a match;
//! - cooldowns reset when a new match starts.

use std::collections::HashMap;

use crate::advice::Advice;
use crate::snapshot::GameSnapshot;

#[derive(Debug, Clone)]
pub struct Coordinator {
    min_gap_secs: i64,
    last_fired: HashMap<String, i64>,
    last_spoken: Option<i64>,
    match_id: Option<String>,
    last_clock: Option<i64>,
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new(3)
    }
}

impl Coordinator {
    pub fn new(min_gap_secs: i64) -> Self {
        Self {
            min_gap_secs,
            last_fired: HashMap::new(),
            last_spoken: None,
            match_id: None,
            last_clock: None,
        }
    }

    /// Returns true when `snapshot` is from a different match than the last
    /// one seen, after resetting cooldowns.
    pub fn observe(&mut self, snapshot: &GameSnapshot) -> bool {
        // A new match id, or the clock jumping back a lot (a different lobby
        // with the same id, or a replay restarting), starts fresh.
        let new_match = self.match_id.as_deref() != Some(snapshot.match_id.as_str())
            || self
                .last_clock
                .is_some_and(|last| snapshot.clock_time + 60 < last);
        if new_match {
            self.last_fired.clear();
            self.last_spoken = None;
            self.match_id = Some(snapshot.match_id.clone());
        }
        self.last_clock = Some(snapshot.clock_time);
        new_match
    }

    /// Picks at most one piece of advice to say now.
    pub fn select(&mut self, snapshot: &GameSnapshot, candidates: Vec<Advice>) -> Option<Advice> {
        if !snapshot.in_game || snapshot.paused {
            return None;
        }
        let now = snapshot.clock_time;
        if self
            .last_spoken
            .is_some_and(|last| now - last < self.min_gap_secs)
        {
            return None;
        }

        let chosen = candidates
            .into_iter()
            .filter(|a| {
                self.last_fired
                    .get(&a.rule_id)
                    .is_none_or(|&last| now - last >= a.cooldown_secs)
            })
            // Highest priority first; ties go to the first rule in load order.
            .reduce(|best, a| if a.priority > best.priority { a } else { best })?;

        self.last_fired.insert(chosen.rule_id.clone(), now);
        self.last_spoken = Some(now);
        Some(chosen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(clock: i64) -> GameSnapshot {
        GameSnapshot {
            match_id: "1".into(),
            in_game: true,
            clock_time: clock,
            ..Default::default()
        }
    }

    fn advice(id: &str, priority: i64, cooldown: i64) -> Advice {
        Advice {
            rule_id: id.into(),
            message: id.into(),
            priority,
            cooldown_secs: cooldown,
        }
    }

    #[test]
    fn most_urgent_wins_and_others_wait() {
        let mut c = Coordinator::new(3);
        c.observe(&at(100));
        let picked = c
            .select(&at(100), vec![advice("a", 1, 60), advice("b", 3, 60)])
            .unwrap();
        assert_eq!(picked.rule_id, "b");
        // Within the global gap nothing is said.
        assert!(c.select(&at(101), vec![advice("a", 1, 60)]).is_none());
        // After the gap, the waiting rule gets its turn.
        assert_eq!(
            c.select(&at(103), vec![advice("a", 1, 60), advice("b", 3, 60)])
                .unwrap()
                .rule_id,
            "a"
        );
    }

    #[test]
    fn respects_per_rule_cooldown() {
        let mut c = Coordinator::new(0);
        c.observe(&at(100));
        assert!(c.select(&at(100), vec![advice("a", 1, 30)]).is_some());
        assert!(c.select(&at(129), vec![advice("a", 1, 30)]).is_none());
        assert!(c.select(&at(130), vec![advice("a", 1, 30)]).is_some());
    }

    #[test]
    fn silent_when_paused_or_out_of_game() {
        let mut c = Coordinator::new(0);
        let mut paused = at(100);
        paused.paused = true;
        assert!(c.select(&paused, vec![advice("a", 1, 0)]).is_none());
        let mut menu = at(100);
        menu.in_game = false;
        assert!(c.select(&menu, vec![advice("a", 1, 0)]).is_none());
    }

    #[test]
    fn new_match_resets_cooldowns() {
        let mut c = Coordinator::new(0);
        assert!(c.observe(&at(100)));
        assert!(c.select(&at(100), vec![advice("a", 1, 600)]).is_some());
        assert!(!c.observe(&at(110)));
        assert!(c.select(&at(110), vec![advice("a", 1, 600)]).is_none());

        let mut next = at(5);
        next.match_id = "2".into();
        assert!(c.observe(&next));
        assert!(c.select(&next, vec![advice("a", 1, 600)]).is_some());
    }
}
