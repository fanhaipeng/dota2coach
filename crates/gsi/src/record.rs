//! Record GSI payloads to a file and play them back later.
//!
//! The file is JSON Lines: one `{"t_ms": <ms since start>, "payload": {...}}`
//! object per payload. Replaying a recording lets you test rules at your desk
//! without launching Dota.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::mpsc;
use tracing::warn;

use crate::model::GameState;
use crate::server::Update;

#[derive(Serialize)]
struct LineOut<'a> {
    t_ms: u64,
    payload: &'a RawValue,
}

#[derive(Deserialize)]
struct LineIn {
    t_ms: u64,
    payload: Box<RawValue>,
}

/// Appends payloads to a JSON Lines file.
pub struct Recorder {
    out: BufWriter<File>,
}

impl Recorder {
    pub async fn create(path: &Path) -> anyhow::Result<Self> {
        let file = File::create(path).await?;
        Ok(Self {
            out: BufWriter::new(file),
        })
    }

    pub async fn write(&mut self, update: &Update) -> anyhow::Result<()> {
        // Keep the payload byte-for-byte, minus newlines so it stays one line.
        let compact =
            serde_json::to_string(&serde_json::from_str::<serde_json::Value>(&update.raw)?)?;
        let payload = RawValue::from_string(compact)?;
        let line = serde_json::to_string(&LineOut {
            t_ms: update.received_ms,
            payload: &payload,
        })?;
        self.out.write_all(line.as_bytes()).await?;
        self.out.write_all(b"\n").await?;
        self.out.flush().await?;
        Ok(())
    }
}

/// Feeds a recording into `tx`, keeping the original gaps between payloads
/// divided by `speed`. A `speed` of 0 or less sends everything immediately.
pub async fn replay(path: &Path, speed: f64, tx: mpsc::Sender<Update>) -> anyhow::Result<usize> {
    let file = File::open(path).await?;
    let mut lines = BufReader::new(file).lines();
    let mut previous_ms: Option<u64> = None;
    let mut sent = 0;

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: LineIn = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(err) => {
                warn!(%err, "skipping a recording line that didn't parse");
                continue;
            }
        };
        if speed > 0.0
            && let Some(prev) = previous_ms
        {
            let gap = parsed.t_ms.saturating_sub(prev) as f64 / speed;
            tokio::time::sleep(Duration::from_millis(gap as u64)).await;
        }
        previous_ms = Some(parsed.t_ms);

        let raw = parsed.payload.get().to_owned();
        let state: GameState = serde_json::from_str(&raw)?;
        let update = Update {
            received_ms: parsed.t_ms,
            raw,
            state,
        };
        if tx.send(update).await.is_err() {
            break;
        }
        sent += 1;
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trips_a_recording() {
        let dir = std::env::temp_dir().join(format!("dota2coach-rec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("game.jsonl");

        let raw = include_str!("../tests/fixtures/player_state.json").to_owned();
        let state: GameState = serde_json::from_str(&raw).unwrap();
        let mut recorder = Recorder::create(&path).await.unwrap();
        for t in [0, 100, 250] {
            let update = Update {
                received_ms: t,
                raw: raw.clone(),
                state: state.clone(),
            };
            recorder.write(&update).await.unwrap();
        }
        drop(recorder);

        let (tx, mut rx) = mpsc::channel(8);
        let sent = replay(&path, 0.0, tx).await.unwrap();
        assert_eq!(sent, 3);
        let mut times = Vec::new();
        while let Ok(update) = rx.try_recv() {
            assert_eq!(update.state, state);
            times.push(update.received_ms);
        }
        assert_eq!(times, vec![0, 100, 250]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
