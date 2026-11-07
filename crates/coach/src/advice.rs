//! What advisors produce, and the trait every advisor implements.

use crate::snapshot::GameSnapshot;

/// One piece of advice an advisor wants to say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Advice {
    /// Stable id, for example `universal/no_tp`. Used for cooldowns and logs.
    pub rule_id: String,
    /// The sentence to speak.
    pub message: String,
    /// Higher is more urgent. Rough scale: 1 = reminder, 2 = should act soon,
    /// 3 = act now (danger).
    pub priority: i64,
    /// Minimum game seconds before this rule may speak again.
    pub cooldown_secs: i64,
    /// What the advice is about, shared across rules, for example
    /// `counter.invis.npc_dota_hero_riki`. Advice whose topics were all said
    /// in the last `topic_cooldown_secs` is skipped, whichever rule said them.
    pub topics: Vec<String>,
    pub topic_cooldown_secs: i64,
}

/// Anything that turns a snapshot into advice: the rule engine today, an LLM
/// advisor or a trained model later. Advisors are stateless with respect to
/// timing; the [`Coordinator`](crate::Coordinator) owns cooldowns and pacing.
pub trait Advisor: Send {
    fn name(&self) -> &str;
    fn advise(&mut self, snapshot: &GameSnapshot) -> Vec<Advice>;
}
