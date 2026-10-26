//! Post-game review, part two: a coach's write-up of the facts.
//!
//! [`Report`] says what happened; a [`Reviewer`] says what mattered. The
//! write-up picks the costliest mistakes, one thing done well and one thing to
//! focus on next game, in the player's language. [`Claude`] writes it with
//! the player's own Anthropic API key; other reviewers can implement the
//! trait.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use anyhow::{Context, bail};
use coach::Report;
use coach::i18n::{Arg, Catalog, LangMeta};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, warn};

/// At most this many mistakes are shown, costliest first.
pub const MAX_MISTAKES: usize = 3;

/// The coach's write-up of one game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Writeup {
    /// How the game went, in a sentence or two.
    pub summary: String,
    /// Costliest first.
    pub mistakes: Vec<Point>,
    pub well_done: Point,
    /// The one habit to work on next game.
    pub focus: Point,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    /// Game clock it refers to, such as `8:20` or `10:00-14:00`; empty if it
    /// is about the whole game.
    pub when: String,
    pub title: String,
    pub detail: String,
}

/// Writes the coach's write-up for a game.
pub trait Reviewer {
    fn write_up(
        &self,
        report: &Report,
        lang: &LangMeta,
    ) -> impl Future<Output = anyhow::Result<Writeup>> + Send;
}

impl Writeup {
    /// The write-up as plain text in the catalog's language.
    pub fn render(&self, catalog: &Catalog) -> String {
        let t = |key: &str| {
            catalog
                .render(&format!("review.{key}"), &BTreeMap::<String, Arg>::new())
                .unwrap_or_else(|| format!("review.{key}"))
        };
        let point = |out: &mut Vec<String>, p: &Point| {
            if p.when.is_empty() {
                out.push(format!("  {}", p.title));
            } else {
                out.push(format!("  {}  {}", p.when, p.title));
            }
            out.extend(p.detail.lines().map(|l| format!("    {l}")));
        };

        let mut out = vec![t("coach_header"), format!("  {}", self.summary)];
        out.push(String::new());
        out.push(t("coach_mistakes"));
        for p in self.mistakes.iter().take(MAX_MISTAKES) {
            point(&mut out, p);
        }
        out.push(String::new());
        out.push(t("coach_well_done"));
        point(&mut out, &self.well_done);
        out.push(String::new());
        out.push(t("coach_focus"));
        point(&mut out, &self.focus);
        out.push(String::new());
        out.join("\n")
    }
}

const API_VERSION: &str = "2026-10-21";
/// Retries a refused request on another model instead of returning the
/// refusal, picked by Anthropic per refusal reason.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Models that accept `fallbacks: "default"`.
const FALLBACK_MODELS: &[&str] = &[
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-fable-5-1",
    "claude-sonnet-5-5",
];
const MAX_TOKENS: u32 = 16_000;
/// Retries after a rate limit, overload or network error.
const RETRIES: u32 = 2;

const SYSTEM_PROMPT: &str = "\
You are an experienced Dota 2 coach reviewing one of your student's public \
games on the current patch (7.41). You get the facts of the game as JSON, \
worked out from Game State Integration, which only sees the student's own hero:

- hero: Dota's internal hero name (zuus is Zeus, nevermore is Shadow Fiend, \
and so on). role: pos1 carry, pos2 mid, pos3 offlane, pos4 soft support, \
pos5 hard support, detected from the laning stage; empty if unknown.
- farm: last hits and GPM at set minutes, with the last-hit target for the \
role where there is one. timeline: the hero at the start of every minute; \
lane is from the student's team's point of view (safe, mid, off), null in the \
jungle, river or a base.
- death_list: each death with where it happened, gold carried and lost, \
whether a TP scroll was in the inventory, and the live coach's warnings in the \
30 seconds before. items: items held at the end and when each was first \
carried. banked: time spent alive carrying more than 2000 unspent gold. \
advice: everything the live voice coach said, with the game clock in seconds.

Write the review the student reads right after the game:
- summary: one or two sentences on how the game went for them.
- mistakes: the costliest mistakes, most costly first, at most three. Only \
what the facts support; fewer is fine. Say when, what it cost (gold, time, \
a fight, the lane) and what to do instead, concretely for this hero and role.
- well_done: one thing they did well, backed by the facts.
- focus: the single habit to work on next game, with a target they can check.

Rules: never invent events, teammates, enemies or numbers that are not in the \
facts; if something can't be told from the facts, leave it out. Write clock \
times as m:ss. Keep each detail to two or three short sentences. Write every \
field in the language you are asked for, using the Dota terms players of that \
language use.";

/// Writes the review with Claude, using the player's own API key.
pub struct Claude {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl Claude {
    pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

    pub fn new(api_key: String, base_url: Option<String>, model: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("HTTP client with default TLS");
        Self {
            http,
            base_url: base_url
                .unwrap_or_else(|| "https://api.anthropic.com".to_owned())
                .trim_end_matches('/')
                .to_owned(),
            api_key,
            model: model.unwrap_or_else(|| Self::DEFAULT_MODEL.to_owned()),
        }
    }

    /// Reads the key from `ANTHROPIC_API_KEY` and an optional
    /// `ANTHROPIC_BASE_URL`.
    pub fn from_env(model: Option<String>) -> anyhow::Result<Self> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .context(
                "the coach's write-up needs an Anthropic API key in ANTHROPIC_API_KEY \
                 (create one in the Claude Console at https://platform.claude.com)",
            )?;
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .ok()
            .filter(|u| !u.trim().is_empty());
        Ok(Self::new(key, base_url, model))
    }

    fn request_body(&self, report: &Report, lang: &LangMeta) -> anyhow::Result<serde_json::Value> {
        let facts = serde_json::to_string(report)?;
        let mut body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": SYSTEM_PROMPT,
            "output_config": {
                "effort": "high",
                "format": { "type": "json_schema", "schema": writeup_schema() },
            },
            "messages": [{
                "role": "user",
                "content": format!(
                    "Review this game. Write in {} (language code {}).\n\n<facts>\n{facts}\n</facts>",
                    lang.name, lang.code
                ),
            }],
        });
        if self.uses_fallbacks() {
            body["fallbacks"] = json!("default");
        }
        Ok(body)
    }

    fn uses_fallbacks(&self) -> bool {
        FALLBACK_MODELS.contains(&self.model.as_str())
    }

    async fn send(&self, body: &serde_json::Value) -> anyhow::Result<MessageResponse> {
        let url = format!("{}/v1/messages", self.base_url);
        let mut attempt = 0;
        loop {
            let mut request = self
                .http
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", API_VERSION)
                .json(body);
            if self.uses_fallbacks() {
                request = request.header("anthropic-beta", FALLBACK_BETA);
            }
            let wait = match request.send().await {
                Ok(response) if response.status().is_success() => {
                    return response
                        .json::<MessageResponse>()
                        .await
                        .context("reading Claude's reply");
                }
                Ok(response) => {
                    let status = response.status();
                    let retry_after = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok());
                    let text = response.text().await.unwrap_or_default();
                    let message = serde_json::from_str::<ErrorResponse>(&text)
                        .map(|e| e.error.message)
                        .unwrap_or(text);
                    let retryable = status.as_u16() == 429 || status.is_server_error();
                    if !retryable || attempt >= RETRIES {
                        match status.as_u16() {
                            401 => bail!("Anthropic rejected the API key: {message}"),
                            403 => bail!("the API key can't use {}: {message}", self.model),
                            _ => bail!("Claude API error {status}: {message}"),
                        }
                    }
                    warn!(%status, %message, "Claude is busy, retrying");
                    retry_after.map_or(2 << attempt, |s| s.min(60))
                }
                Err(err) if attempt < RETRIES && (err.is_connect() || err.is_timeout()) => {
                    warn!(%err, "couldn't reach Claude, retrying");
                    2 << attempt
                }
                Err(err) => return Err(err).context("couldn't reach the Claude API"),
            };
            attempt += 1;
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
    }
}

impl Reviewer for Claude {
    async fn write_up(&self, report: &Report, lang: &LangMeta) -> anyhow::Result<Writeup> {
        let body = self.request_body(report, lang)?;
        let reply = self.send(&body).await?;
        debug!(model = %reply.model, stop = ?reply.stop_reason, usage = ?reply.usage, "write-up");
        match reply.stop_reason.as_deref() {
            Some("refusal") => bail!("Claude declined to review this game"),
            Some("max_tokens") => bail!("Claude's write-up was cut off; try again"),
            _ => {}
        }
        let text: String = reply
            .content
            .iter()
            .filter(|b| b.kind == "text")
            .filter_map(|b| b.text.as_deref())
            .collect();
        serde_json::from_str(&text).with_context(|| format!("unexpected write-up: {text}"))
    }
}

/// The JSON schema Claude's reply must follow.
fn writeup_schema() -> serde_json::Value {
    let point = json!({
        "type": "object",
        "properties": {
            "when": { "type": "string" },
            "title": { "type": "string" },
            "detail": { "type": "string" },
        },
        "required": ["when", "title", "detail"],
        "additionalProperties": false,
    });
    json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "mistakes": { "type": "array", "items": point },
            "well_done": point,
            "focus": point,
        },
        "required": ["summary", "mistakes", "well_done", "focus"],
        "additionalProperties": false,
    })
}

#[derive(Debug, Deserialize)]
struct MessageResponse {
    #[serde(default)]
    model: String,
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    usage: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    message: String,
}
