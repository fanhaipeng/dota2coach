//! Dota 2 Game State Integration: payload model, local HTTP receiver, and
//! record/replay of payloads.

pub mod model;
pub mod record;
pub mod server;

pub use model::GameState;
pub use server::Update;
