//! The coach's write-up against a stand-in for the Claude API.

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use coach::review::analyze;
use coach::{Catalog, GameSnapshot, LangMeta, Report};
use dota2coach::writeup::{Claude, Point, Reviewer, Writeup};
use serde_json::{Value, json};

#[derive(Default)]
struct Seen {
    requests: Vec<(HeaderMap, Value)>,
}

type Replies = Arc<Mutex<Vec<(StatusCode, Value)>>>;

/// Serves `replies` in order and records every request.
async fn fake_api(replies: Vec<(StatusCode, Value)>) -> (String, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let replies: Replies = Arc::new(Mutex::new(replies));
    let state = (seen.clone(), replies);
    let app = axum::Router::new()
        .route(
            "/v1/messages",
            post(
                |State((seen, replies)): State<(Arc<Mutex<Seen>>, Replies)>,
                 headers: HeaderMap,
                 Json(body): Json<Value>| async move {
                    seen.lock().unwrap().requests.push((headers, body));
                    let (status, reply) = replies.lock().unwrap().remove(0);
                    let mut response: Response = (status, Json(reply)).into_response();
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        response
                            .headers_mut()
                            .insert("retry-after", "0".parse().unwrap());
                    }
                    response
                },
            ),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

fn report() -> Report {
    let game: Vec<GameSnapshot> = (0..=600)
        .map(|t| GameSnapshot {
            match_id: "42".into(),
            in_game: true,
            clock_time: t,
            team: "radiant".into(),
            role: "pos2".into(),
            hero: "sniper".into(),
            alive: true,
            last_hits: t / 15,
            ..Default::default()
        })
        .collect();
    analyze(&game, &[]).unwrap()
}

fn writeup() -> Writeup {
    let point = |when: &str, title: &str| Point {
        when: when.into(),
        title: title.into(),
        detail: "Line one.\nLine two.".into(),
    };
    Writeup {
        summary: "A slow lane, then a steady game.".into(),
        mistakes: vec![point("5:00", "Missed last hits"), point("", "Rarely moved")],
        well_done: point("", "Never died"),
        focus: point("", "40 last hits by 10:00"),
    }
}

fn message(stop: &str, text: &str) -> Value {
    json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-opus-5-5",
        "content": [
            { "type": "thinking", "thinking": "", "signature": "x" },
            { "type": "text", "text": text },
        ],
        "stop_reason": stop,
        "usage": { "input_tokens": 1000, "output_tokens": 200 },
    })
}

fn chinese() -> LangMeta {
    let rules = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules");
    Catalog::load("zh", &[rules]).unwrap().meta().clone()
}

#[tokio::test]
async fn asks_claude_for_a_structured_write_up() {
    let expected = writeup();
    let text = serde_json::to_string(&expected).unwrap();
    let (url, seen) = fake_api(vec![
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({ "type": "error", "error": { "type": "rate_limit_error", "message": "slow down" } }),
        ),
        (StatusCode::OK, message("end_turn", &text)),
    ])
    .await;

    let claude = Claude::new("test-key".into(), Some(url), None);
    let got = claude.write_up(&report(), &chinese()).await.unwrap();
    assert_eq!(got, expected);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.requests.len(), 2, "retried after the rate limit");
    let (headers, body) = &seen.requests[1];
    assert_eq!(headers["x-api-key"], "test-key");
    assert_eq!(headers["anthropic-version"], "2026-10-21");
    assert_eq!(headers["anthropic-beta"], "server-side-fallback-2026-07-01");
    assert_eq!(body["model"], "claude-opus-5-5");
    assert_eq!(body["fallbacks"], "default");
    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    let prompt = body["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains("Write in 中文"), "{prompt}");
    assert!(prompt.contains("\"hero\":\"sniper\""), "{prompt}");
}

#[tokio::test]
async fn explains_api_failures() {
    let (url, _) = fake_api(vec![
        (
            StatusCode::UNAUTHORIZED,
            json!({ "type": "error", "error": { "type": "authentication_error", "message": "invalid x-api-key" } }),
        ),
        (StatusCode::OK, message("refusal", "")),
    ])
    .await;
    let claude = Claude::new("bad".into(), Some(url), Some("claude-haiku-5-5".into()));

    let err = claude.write_up(&report(), &chinese()).await.unwrap_err();
    assert!(err.to_string().contains("rejected the API key"), "{err}");
    let err = claude.write_up(&report(), &chinese()).await.unwrap_err();
    assert!(err.to_string().contains("declined"), "{err}");
}

#[test]
fn renders_the_write_up() {
    let rules = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules");
    let text = writeup().render(&Catalog::load("en", &[rules]).unwrap());
    assert!(text.starts_with("Coach's review\n  A slow lane"), "{text}");
    assert!(
        text.contains("  5:00  Missed last hits\n    Line one.\n    Line two."),
        "{text}"
    );
    assert!(
        text.contains("Focus next game\n  40 last hits by 10:00"),
        "{text}"
    );
}
