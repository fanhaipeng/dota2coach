//! The coach: turns game snapshots into a paced stream of spoken advice.
//!
//! - [`GameSnapshot`] is the flat view of the game every advisor reads.
//! - [`Advisor`] is implemented by [`RuleEngine`] today, and by an LLM advisor
//!   or a trained model later.
//! - [`Coordinator`] owns cooldowns and pacing, so advisors stay simple.

pub mod advice;
pub mod coordinator;
pub mod rules;
pub mod snapshot;

pub use advice::{Advice, Advisor};
pub use coordinator::Coordinator;
pub use rules::{Packs, RuleEngine};
pub use snapshot::GameSnapshot;
