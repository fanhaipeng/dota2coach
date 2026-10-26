use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use coach::Catalog;
use dota2coach::writeup::{Claude, Reviewer};
use dota2coach::{
    CoachConfig, Recording, latest_recording, resolve_rules_dir, review_file, run_coach, steam,
};
use gsi::record::{self, Recorder};
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use voice::{ConsoleSpeaker, Speaker, VoiceRequest};

const DEFAULT_PORT: u16 = 3000;
const DEFAULT_TOKEN: &str = "dota2coach-local";
const DEFAULT_GAMES_DIR: &str = "games";

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
    /// Role pack to load from rules/roles/, for example pos1 or pos3. Without
    /// it, the role is detected from where you lane in the first 4 minutes.
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
        /// Save everything to this one file instead of one file per match.
        #[arg(long)]
        record: Option<PathBuf>,
        /// Folder where each match is recorded for `review`.
        #[arg(long, default_value = DEFAULT_GAMES_DIR)]
        games: PathBuf,
        /// Don't record anything.
        #[arg(long, conflicts_with = "record")]
        no_record: bool,
    },
    /// Review a recorded game: deaths, farm, items, unspent gold and advice.
    Review {
        /// A recording; defaults to the newest one in the games folder.
        file: Option<PathBuf>,
        #[arg(long, default_value = DEFAULT_GAMES_DIR)]
        games: PathBuf,
        #[arg(long)]
        rules: Option<PathBuf>,
        /// Language of the review, for example en or zh.
        #[arg(long, default_value = "en")]
        lang: String,
        /// The role you played, if detection got it wrong.
        #[arg(long)]
        role: Option<String>,
        /// Print the facts as JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Add a coach's write-up by Claude: your costliest mistakes, one thing
        /// done well and what to focus on next game. Needs ANTHROPIC_API_KEY.
        #[arg(long)]
        coach: bool,
        /// Claude model for the write-up.
        #[arg(long, default_value = Claude::DEFAULT_MODEL, requires = "coach")]
        model: String,
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
    /// Find Dota 2 and install the GSI config file, so Dota sends game state
    /// to the coach.
    Setup {
        /// The Dota 2 install folder ("dota 2 beta"), if it isn't found
        /// automatically. In Steam: Dota 2 > Properties > Installed Files >
        /// Browse.
        #[arg(long)]
        dota_dir: Option<PathBuf>,
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
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
    let cli = Cli::parse();
    // A review prints its report; the coach's play-by-play would bury it.
    let level = if matches!(cli.command, Cmd::Review { .. }) {
        "warn"
    } else {
        "info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level)),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    match cli.command {
        Cmd::Setup {
            dota_dir,
            port,
            token,
        } => setup(dota_dir, port, &token)?,
        Cmd::GsiConfig { port, token } => {
            print!("{}", gsi::server::config_file(port, &token));
        }
        Cmd::Run {
            coach,
            port,
            token,
            record,
            games,
            no_record,
        } => {
            let recording = match &record {
                Some(path) => Recording::File(Recorder::create(path).await?),
                None if no_record => Recording::Off,
                None => Recording::per_match(games),
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
                result = run_coach(rx, coach.config(), speaker.as_ref(), recording, None) => {
                    result?;
                }
                _ = tokio::signal::ctrl_c() => info!("stopping"),
            }
        }
        Cmd::Replay { file, coach, speed } => {
            let (tx, rx) = mpsc::channel(256);
            let reader = tokio::spawn(async move { record::replay(&file, speed, tx).await });
            let speaker = coach.speaker()?;
            let spoken =
                run_coach(rx, coach.config(), speaker.as_ref(), Recording::Off, None).await?;
            let payloads = reader.await??;
            info!(payloads, messages = spoken.len(), "replay finished");
        }
        Cmd::Review {
            file,
            games,
            rules,
            lang,
            role,
            json,
            coach,
            model,
        } => {
            // Fail on a missing key before spending time on the replay.
            let reviewer = coach.then(|| Claude::from_env(Some(model))).transpose()?;
            let file = match file.or_else(|| latest_recording(&games)) {
                Some(file) => file,
                None => anyhow::bail!(
                    "no recordings in {}; play a game with `dota2coach run` first",
                    games.display()
                ),
            };
            let config = CoachConfig {
                rules_dir: resolve_rules_dir(rules.as_deref()),
                role,
                min_gap_secs: 3,
                lang,
            };
            let catalog = Catalog::load(&config.lang, std::slice::from_ref(&config.rules_dir))?;
            info!(file = %file.display(), "reviewing");
            let Some(report) = review_file(&file, config).await? else {
                anyhow::bail!("{} has no match in it", file.display());
            };
            if !json {
                print!("{}", report.render(&catalog));
            }
            let writeup = match &reviewer {
                Some(reviewer) => {
                    eprintln!("\nAsking the coach for a write-up...");
                    Some(reviewer.write_up(&report, catalog.meta()).await?)
                }
                None => None,
            };
            match (json, writeup) {
                (true, None) => println!("{}", serde_json::to_string_pretty(&report)?),
                (true, Some(writeup)) => println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "facts": report,
                        "coach": writeup,
                    }))?
                ),
                (false, Some(writeup)) => print!("\n{}", writeup.render(&catalog)),
                (false, None) => {}
            }
        }
    }
    Ok(())
}

fn setup(dota_dir: Option<PathBuf>, port: u16, token: &str) -> anyhow::Result<()> {
    let dota = match dota_dir.or_else(steam::find_dota) {
        Some(dir) => dir,
        None => anyhow::bail!(
            "couldn't find Dota 2. In Steam, open Dota 2 > Properties > Installed Files > \
             Browse, then run: dota2coach setup --dota-dir \"<that folder>\""
        ),
    };
    if !dota.join("game").is_dir() {
        anyhow::bail!(
            "{} doesn't look like a Dota 2 install (no `game` folder inside)",
            dota.display()
        );
    }
    let dir = steam::gsi_dir(&dota);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let file = dir.join("gamestate_integration_dota2coach.cfg");
    // Plain ASCII: Dota ignores the UTF-16 files that PowerShell's `>` writes.
    std::fs::write(&file, gsi::server::config_file(port, token))
        .with_context(|| format!("writing {}", file.display()))?;
    println!("Installed the GSI config: {}", file.display());
    println!();
    println!("Next:");
    println!("  1. In Steam, add -gamestateintegration to Dota 2 > Properties > Launch Options.");
    println!("  2. Restart Dota 2 if it is running (it reads the config at startup).");
    println!("  3. Run: dota2coach run");
    Ok(())
}
