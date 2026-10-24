//! Spoken output, using whatever speech the OS already has.
//!
//! - Windows: one long-lived PowerShell process driving `System.Speech`.
//! - macOS: the built-in `say` command.
//! - Linux: `spd-say` or `espeak-ng`, if installed.
//!
//! If none is available the coach falls back to printing. Speech runs on its
//! own thread so the coach never waits for it, and a message that has waited
//! too long in the queue is dropped instead of being said late.

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tracing::{info, warn};

/// Messages older than this when their turn comes are skipped.
const MAX_QUEUE_AGE: Duration = Duration::from_secs(8);

pub trait Speaker: Send + Sync {
    fn say(&self, text: &str);
}

/// Prints messages instead of speaking them (`--mute`, tests, or no speech
/// engine on this machine).
#[derive(Debug, Default, Clone, Copy)]
pub struct ConsoleSpeaker;

impl Speaker for ConsoleSpeaker {
    fn say(&self, text: &str) {
        println!("[coach] {text}");
    }
}

/// Speaks through the operating system's speech engine on a background thread.
pub struct SystemSpeaker {
    tx: mpsc::Sender<(Instant, String)>,
}

impl SystemSpeaker {
    /// Starts the speech thread. Returns `None` if this machine has no speech
    /// engine the coach knows how to use.
    pub fn start() -> Option<Self> {
        let mut backend = Backend::detect()?;
        info!(backend = backend.name(), "speech ready");
        let (tx, rx) = mpsc::channel::<(Instant, String)>();
        thread::Builder::new()
            .name("speech".into())
            .spawn(move || {
                for (queued, text) in rx {
                    if queued.elapsed() > MAX_QUEUE_AGE {
                        continue;
                    }
                    if let Err(err) = backend.speak(&text) {
                        warn!(%err, "speech failed; printing instead");
                        ConsoleSpeaker.say(&text);
                    }
                }
            })
            .ok()?;
        Some(Self { tx })
    }
}

impl Speaker for SystemSpeaker {
    fn say(&self, text: &str) {
        let _ = self.tx.send((Instant::now(), sanitize(text)));
    }
}

/// The system speaker if available, otherwise the console.
pub fn default_speaker() -> Box<dyn Speaker> {
    match SystemSpeaker::start() {
        Some(speaker) => Box::new(speaker),
        None => {
            warn!("no speech engine found; advice will be printed");
            Box::new(ConsoleSpeaker)
        }
    }
}

/// One line of plain text: speech engines and the PowerShell pipe both read
/// line by line.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

enum Backend {
    /// A PowerShell process that speaks each line written to its stdin.
    #[cfg_attr(not(windows), allow(dead_code))]
    PowerShell { _child: Child, stdin: ChildStdin },
    /// A command run once per message, with the text as its last argument.
    #[cfg_attr(windows, allow(dead_code))]
    Command {
        program: &'static str,
        args: &'static [&'static str],
    },
}

impl Backend {
    fn detect() -> Option<Self> {
        #[cfg(windows)]
        {
            Self::powershell()
        }
        #[cfg(target_os = "macos")]
        {
            Some(Backend::Command {
                program: "say",
                args: &[],
            })
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // `-w` waits until speaking finishes, so messages don't overlap.
            [
                ("spd-say", &["-w"][..]),
                ("espeak-ng", &[][..]),
                ("espeak", &[][..]),
            ]
            .into_iter()
            .find(|(program, _)| on_path(program))
            .map(|(program, args)| Backend::Command { program, args })
        }
    }

    #[cfg(windows)]
    fn powershell() -> Option<Self> {
        const SCRIPT: &str = "Add-Type -AssemblyName System.Speech; \
            $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
            while (($line = [Console]::In.ReadLine()) -ne $null) { $s.Speak($line) }";
        let mut child = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdin = child.stdin.take()?;
        Some(Backend::PowerShell {
            _child: child,
            stdin,
        })
    }

    fn name(&self) -> &'static str {
        match self {
            Backend::PowerShell { .. } => "powershell System.Speech",
            Backend::Command { program, .. } => program,
        }
    }

    fn speak(&mut self, text: &str) -> std::io::Result<()> {
        match self {
            Backend::PowerShell { stdin, .. } => {
                writeln!(stdin, "{text}")?;
                stdin.flush()
            }
            Backend::Command { program, args } => {
                Command::new(*program)
                    .args(*args)
                    .arg(text)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()?;
                Ok(())
            }
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_makes_one_clean_line() {
        assert_eq!(sanitize("  Buy a TP\nscroll.\r\n"), "Buy a TP scroll.");
        assert_eq!(sanitize("Rune\tin 15 seconds"), "Rune in 15 seconds");
    }

    #[test]
    fn default_speaker_always_returns_something() {
        // On a machine without a speech engine this is the console fallback.
        let speaker = default_speaker();
        speaker.say("test");
    }
}
