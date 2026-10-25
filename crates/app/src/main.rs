use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use coach::Catalog;
use dota2coach::{CoachConfig, resolve_rules_dir, run_coach};
use gsi::record::{self, Recorder};
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use voice::{ConsoleSpeaker, Speaker, VoiceRequest};

const DEFAULT_PORT: u16 = 3000;
const DEFAULT_TOKEN: &str = "dota2coach-local";

/// A voice coach for Dota 2, driven by Game State Integration.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(clap::Args)]
struct CoachArgs {
    /// Rules folder. Defaults to ./rules, then rules/ next to the executable.
    #[arg(long)]
    rules: Option<PathBuf>,
    /// Role pack to load from rules/roles/, for example pos1 or pos3.
    #[arg(long)]
    role: Option<String>,
    /// Minimum game seconds between two spoken messages.
    #[arg(long, default_value_t = 3)]
    gap: i64,
    /// Language to coach in: a file name from rules/lang/, for example en or zh.
    #[arg(long, default_value = "en")]
    lang: String,
    /// Speak with this installed voice instead of the language's default one.
    #[arg(long)]
    voice: Option<String>,
    /// Print advice instead of speaking it.
    #[arg(long)]
    mute: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Listen for Dota 2 and coach live.
    Run {
        #[command(flatten)]
        coach: CoachArgs,
        /// Port the GSI config points at.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
        /// Auth token from the GSI config; other payloads are ignored.
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
        /// Also save every payload to this file, for replaying later.
        #[arg(long)]
        record: Option<PathBuf>,
    },
    /// Coach a recorded game, without Dota running.
    Replay {
        /// A recording made with `run --record`.
        file: PathBuf,
        #[command(flatten)]
        coach: CoachArgs,
        /// Playback speed; 0 replays as fast as possible.
        #[arg(long, default_value_t = 1.0)]
        speed: f64,
    },
    /// Print the GSI config file to put in Dota's gamestate_integration folder.
    GsiConfig {
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
}

impl CoachArgs {
    fn config(&self) -> CoachConfig {
        CoachConfig {
            rules_dir: resolve_rules_dir(self.rules.as_deref()),
            role: self.role.clone(),
            min_gap_secs: self.gap,
            lang: self.lang.clone(),
        }
    }

    /// Fails early, before Dota or a replay starts, if the language doesn't
    /// exist.
    fn speaker(&self) -> anyhow::Result<Box<dyn Speaker>> {
        let config = self.config();
        let catalog = Catalog::load(&config.lang, std::slice::from_ref(&config.rules_dir))?;
        if self.mute {
            return Ok(Box::new(ConsoleSpeaker));
        }
        Ok(voice::default_speaker(&VoiceRequest {
            lang: catalog.meta().voice.clone(),
            name: self.voice.clone(),
        }))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    match Cli::parse().command {
        Cmd::GsiConfig { port, token } => {
            print!("{}", gsi::server::config_file(port, &token));
        }
        Cmd::Run {
            coach,
            port,
            token,
            record,
        } => {
            let recorder = match &record {
                Some(path) => Some(Recorder::create(path).await?),
                None => None,
            };
            let (tx, rx) = mpsc::channel(256);
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            info!(%addr, "waiting for Dota 2; start a game to begin");
            tokio::spawn(async move {
                if let Err(err) = gsi::server::serve(addr, Some(token), tx).await {
                    tracing::error!(%err, "GSI server stopped");
                }
            });
            let speaker = coach.speaker()?;
            tokio::select! {
                result = run_coach(rx, coach.config(), speaker.as_ref(), recorder) => {
                    result?;
                }
                _ = tokio::signal::ctrl_c() => info!("stopping"),
            }
        }
        Cmd::Replay { file, coach, speed } => {
            let (tx, rx) = mpsc::channel(256);
            let reader = tokio::spawn(async move { record::replay(&file, speed, tx).await });
            let speaker = coach.speaker()?;
            let spoken = run_coach(rx, coach.config(), speaker.as_ref(), None).await?;
            let payloads = reader.await??;
            info!(payloads, messages = spoken.len(), "replay finished");
        }
    }
    Ok(())
}
