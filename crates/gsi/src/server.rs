//! The local HTTP endpoint Dota 2 posts GSI payloads to.

use std::net::SocketAddr;
use std::time::Instant;

use axum::{Router, body::Bytes, extract::State, http::StatusCode, routing::post};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::model::GameState;

/// A payload as it arrived, plus its parsed form.
#[derive(Debug, Clone)]
pub struct Update {
    /// Milliseconds since the server started, used when recording.
    pub received_ms: u64,
    /// The raw JSON body, kept so it can be recorded unchanged.
    pub raw: String,
    pub state: GameState,
}

#[derive(Clone)]
struct AppState {
    token: Option<String>,
    started: Instant,
    tx: mpsc::Sender<Update>,
}

/// Serve the GSI endpoint on `addr` and forward every accepted payload to `tx`.
///
/// When `token` is set, payloads whose `auth.token` doesn't match are dropped,
/// so only the Dota client configured with that token can talk to the coach.
pub async fn serve(
    addr: SocketAddr,
    token: Option<String>,
    tx: mpsc::Sender<Update>,
) -> anyhow::Result<()> {
    let state = AppState {
        token,
        started: Instant::now(),
        tx,
    };
    let app = Router::new().route("/", post(receive)).with_state(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn receive(State(app): State<AppState>, body: Bytes) -> StatusCode {
    let raw = match String::from_utf8(body.to_vec()) {
        Ok(raw) => raw,
        Err(_) => return StatusCode::BAD_REQUEST,
    };
    let state: GameState = match serde_json::from_str(&raw) {
        Ok(state) => state,
        Err(err) => {
            warn!(%err, "dropping a GSI payload that didn't parse");
            return StatusCode::BAD_REQUEST;
        }
    };
    if let Some(expected) = &app.token
        && state.token() != Some(expected.as_str())
    {
        warn!("dropping a GSI payload with the wrong auth token");
        return StatusCode::UNAUTHORIZED;
    }
    let update = Update {
        received_ms: app.started.elapsed().as_millis() as u64,
        raw,
        state,
    };
    if app.tx.send(update).await.is_err() {
        debug!("coach loop has stopped; ignoring payload");
    }
    // Dota retries a payload until it gets a 2xx, so always acknowledge.
    StatusCode::OK
}

/// The GSI config file Dota 2 needs, ready to save as
/// `gamestate_integration_dota2coach.cfg` in the game's
/// `game/dota/cfg/gamestate_integration/` folder.
pub fn config_file(port: u16, token: &str) -> String {
    format!(
        r#""dota2coach"
{{
    "uri"           "http://127.0.0.1:{port}/"
    "timeout"       "5.0"
    "buffer"        "0.1"
    "throttle"      "0.1"
    "heartbeat"     "30.0"
    "data"
    {{
        "provider"      "1"
        "map"           "1"
        "player"        "1"
        "hero"          "1"
        "abilities"     "1"
        "items"         "1"
    }}
    "auth"
    {{
        "token"         "{token}"
    }}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_points_at_the_local_port() {
        let cfg = config_file(3123, "secret");
        assert!(cfg.contains(r#""uri"           "http://127.0.0.1:3123/""#));
        assert!(cfg.contains(r#""token"         "secret""#));
    }

    #[tokio::test]
    async fn accepts_matching_token_and_rejects_others() {
        let (tx, mut rx) = mpsc::channel(4);
        let app = AppState {
            token: Some("dota2coach-local".into()),
            started: Instant::now(),
            tx,
        };
        let good = include_str!("../tests/fixtures/player_state.json");
        let bad = good.replace("dota2coach-local", "someone-else");

        let status = receive(State(app.clone()), Bytes::from(good)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(rx.recv().await.unwrap().state.map.unwrap().clock_time, 754);

        let status = receive(State(app.clone()), Bytes::from(bad)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            receive(State(app), Bytes::from_static(b"not json")).await,
            StatusCode::BAD_REQUEST
        );
        assert!(rx.try_recv().is_err());
    }
}
