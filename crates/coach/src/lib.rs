//! The coach: turns game snapshots into a paced stream of spoken advice.
//!
//! - [`GameSnapshot`] is the flat view of the game every advisor reads.
//! - [`Advisor`] is implemented by [`RuleEngine`] today, and by an LLM advisor
//!   or a trained model later.
//! - [`Coordinator`] owns cooldowns and pacing, so advisors stay simple.
//! - [`RoleDetector`] works out pos1 to pos5 from where the hero lanes.
//! - [`Catalog`] turns rule message keys into sentences in the chosen language.

pub mod advice;
pub mod coordinator;
pub mod history;
pub mod i18n;
pub mod role;
pub mod rules;
pub mod snapshot;

pub use advice::{Advice, Advisor};
pub use coordinator::Coordinator;
pub use history::History;
pub use i18n::{Catalog, LangMeta};
pub use role::{Detection, Lane, RoleDetector};
pub use rules::{Packs, RuleEngine};
pub use snapshot::GameSnapshot;
