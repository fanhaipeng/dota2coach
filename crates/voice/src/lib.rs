//! Spoken output, using whatever speech the OS already has.
//!
//! - Windows: one long-lived PowerShell process driving `System.Speech`.
//! - macOS: the built-in `say` command.
//! - Linux: `spd-say` or `espeak-ng`, if installed.
//!
//! If none is available the coach falls back to printing. Speech runs on its
//! own thread so the coach never waits for it, and a message that has waited
//! too long in the queue is dropped instead of being said late.
//!
//! A [`VoiceRequest`] picks the voice: a language tag such as `zh-CN` (from the
//! message catalog) chooses an installed voice for that language, and an
//! explicit voice name overrides it.

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

/// Which voice to speak with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceRequest {
    /// BCP 47 language tag, for example `en-US` or `zh-CN`.
    pub lang: String,
    /// An installed voice's exact name, overriding the language match.
    pub name: Option<String>,
}

impl VoiceRequest {
    pub fn for_lang(lang: &str) -> Self {
        Self {
            lang: lang.to_owned(),
            name: None,
        }
    }
}

/// Speaks through the operating system's speech engine on a background thread.
pub struct SystemSpeaker {
    tx: mpsc::Sender<(Instant, String)>,
}

impl SystemSpeaker {
    /// Starts the speech thread. Returns `None` if this machine has no speech
    /// engine the coach knows how to use.
    pub fn start(request: &VoiceRequest) -> Option<Self> {
        let mut backend = Backend::detect(request)?;
        info!(
            backend = backend.name(),
            lang = request.lang,
            "speech ready"
        );
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
pub fn default_speaker(request: &VoiceRequest) -> Box<dyn Speaker> {
    match SystemSpeaker::start(request) {
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

/// Picks the installed voice for `request`: the named one if given, else the
/// first whose locale matches the language tag exactly, else the first with the
/// same language (`zh-TW` for `zh-CN`). `voices` are `(name, locale)` pairs;
/// locales may use `_` (macOS) or `-` (Windows).
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn choose_voice(voices: &[(String, String)], request: &VoiceRequest) -> Option<String> {
    if let Some(name) = &request.name {
        return voices
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(n, _)| n.clone());
    }
    let norm = |tag: &str| tag.replace('_', "-").to_ascii_lowercase();
    let want = norm(&request.lang);
    let primary = |tag: &str| tag.split('-').next().unwrap_or_default().to_owned();
    voices
        .iter()
        .find(|(_, locale)| norm(locale) == want)
        .or_else(|| {
            voices
                .iter()
                .find(|(_, locale)| primary(&norm(locale)) == primary(&want))
        })
        .map(|(name, _)| name.clone())
}

/// The language part of a tag: `zh-CN` -> `zh`.
#[cfg_attr(windows, allow(dead_code))]
fn primary_lang(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Warns when the language has no voice, so the user knows why it sounds wrong.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn warn_no_voice(request: &VoiceRequest, voices: &[(String, String)], hint: &str) {
    let installed: Vec<String> = voices.iter().map(|(n, l)| format!("{n} ({l})")).collect();
    match &request.name {
        Some(name) => warn!(
            voice = name,
            installed = installed.join(", "),
            "voice not found; using the default voice"
        ),
        None => warn!(
            lang = request.lang,
            installed = installed.join(", "),
            "no voice for this language; using the default voice. {hint}"
        ),
    }
}

/// `say -v ?` lines look like `Tingting    zh_CN    # 你好，我叫婷婷。`; names
/// can contain spaces.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_say_voices(listing: &str) -> Vec<(String, String)> {
    listing
        .lines()
        .filter_map(|line| {
            let left = line.split('#').next()?.trim_end();
            let (name, locale) = left.rsplit_once(char::is_whitespace)?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_owned(), locale.to_owned()))
        })
        .collect()
}

/// `name|culture` lines from the Windows voice listing.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_pipe_voices(listing: &str) -> Vec<(String, String)> {
    listing
        .lines()
        .filter_map(|line| {
            let (name, culture) = line.trim().split_once('|')?;
            Some((name.to_owned(), culture.to_owned()))
        })
        .collect()
}

enum Backend {
    /// A PowerShell process that speaks each line written to its stdin.
    #[cfg_attr(not(windows), allow(dead_code))]
    PowerShell { _child: Child, stdin: ChildStdin },
    /// A command run once per message, with the text as its last argument.
    #[cfg_attr(windows, allow(dead_code))]
    Command {
        program: &'static str,
        args: Vec<String>,
    },
}

impl Backend {
    fn detect(request: &VoiceRequest) -> Option<Self> {
        #[cfg(windows)]
        {
            Self::powershell(request)
        }
        #[cfg(target_os = "macos")]
        {
            Some(Self::say(request))
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // `-w` waits until speaking finishes, so messages don't overlap.
            // espeak names Mandarin `cmn`.
            let lang = primary_lang(&request.lang);
            let espeak_voice = request.name.clone().unwrap_or_else(|| match lang.as_str() {
                "zh" => "cmn".to_owned(),
                _ => lang.clone(),
            });
            let candidates: [(&'static str, Vec<String>); 3] = [
                ("spd-say", vec!["-w".into(), "-l".into(), lang.clone()]),
                ("espeak-ng", vec!["-v".into(), espeak_voice.clone()]),
                ("espeak", vec!["-v".into(), espeak_voice]),
            ];
            candidates
                .into_iter()
                .find(|(program, _)| on_path(program))
                .map(|(program, args)| Backend::Command { program, args })
        }
    }

    #[cfg(target_os = "macos")]
    fn say(request: &VoiceRequest) -> Self {
        let voices = Command::new("say")
            .args(["-v", "?"])
            .output()
            .map(|out| parse_say_voices(&String::from_utf8_lossy(&out.stdout)))
            .unwrap_or_default();
        // For English keep the voice chosen in System Settings: the voice list
        // starts with novelty voices, which a language match would pick.
        let wants_default = request.name.is_none() && primary_lang(&request.lang) == "en";
        let mut args = Vec::new();
        if !wants_default {
            match choose_voice(&voices, request) {
                Some(name) => {
                    info!(voice = name, "using voice");
                    args = vec!["-v".to_owned(), name];
                }
                None => warn_no_voice(
                    request,
                    &voices,
                    "Add one in System Settings > Accessibility > Spoken Content > System Voice > Manage Voices.",
                ),
            }
        }
        Backend::Command {
            program: "say",
            args,
        }
    }

    #[cfg(windows)]
    fn powershell(request: &VoiceRequest) -> Option<Self> {
        const LIST: &str = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
            Add-Type -AssemblyName System.Speech; \
            (New-Object System.Speech.Synthesis.SpeechSynthesizer).GetInstalledVoices() | \
            Where-Object { $_.Enabled } | \
            ForEach-Object { $_.VoiceInfo.Name + '|' + $_.VoiceInfo.Culture.Name }";
        let voices = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", LIST])
            .stderr(Stdio::null())
            .output()
            .map(|out| parse_pipe_voices(&String::from_utf8_lossy(&out.stdout)))
            .unwrap_or_default();
        let select = match choose_voice(&voices, request) {
            Some(name) => {
                info!(voice = name, "using voice");
                format!("$s.SelectVoice('{}'); ", name.replace('\'', "''"))
            }
            None => {
                warn_no_voice(
                    request,
                    &voices,
                    "Add one in Settings > Time & language > Speech > Add voices, then restart the coach.",
                );
                String::new()
            }
        };
        // Read stdin as UTF-8: the console code page would garble anything
        // that isn't ASCII, such as Chinese.
        let script = format!(
            "Add-Type -AssemblyName System.Speech; \
            $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; {select}\
            $in = New-Object System.IO.StreamReader([Console]::OpenStandardInput(), (New-Object System.Text.UTF8Encoding $false)); \
            while (($line = $in.ReadLine()) -ne $null) {{ $s.Speak($line) }}"
        );
        let mut child = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
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
                    .args(args.iter())
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

    fn voices(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, l)| ((*n).to_owned(), (*l).to_owned()))
            .collect()
    }

    #[test]
    fn chooses_a_voice_for_the_language() {
        let installed = voices(&[
            ("Microsoft David Desktop", "en-US"),
            ("Microsoft Tracy Desktop", "zh-HK"),
            ("Microsoft Huihui Desktop", "zh-CN"),
        ]);
        let pick = |lang: &str| choose_voice(&installed, &VoiceRequest::for_lang(lang));
        assert_eq!(pick("zh-CN").as_deref(), Some("Microsoft Huihui Desktop"));
        assert_eq!(pick("zh-TW").as_deref(), Some("Microsoft Tracy Desktop"));
        assert_eq!(pick("en-GB").as_deref(), Some("Microsoft David Desktop"));
        assert_eq!(pick("fr-FR"), None);

        let named = VoiceRequest {
            lang: "zh-CN".into(),
            name: Some("microsoft david desktop".into()),
        };
        assert_eq!(
            choose_voice(&installed, &named).as_deref(),
            Some("Microsoft David Desktop")
        );
    }

    #[test]
    fn parses_voice_listings() {
        let say = "Albert              en_US    # Hello! My name is Albert.\n\
                   Eddy (Chinese (China mainland)) zh_CN    # 你好！我叫Eddy。\n\
                   Tingting            zh_CN    # 你好，我叫婷婷。\n";
        assert_eq!(
            parse_say_voices(say),
            voices(&[
                ("Albert", "en_US"),
                ("Eddy (Chinese (China mainland))", "zh_CN"),
                ("Tingting", "zh_CN"),
            ])
        );
        assert_eq!(
            parse_pipe_voices("Microsoft Huihui Desktop|zh-CN\r\nMicrosoft Zira Desktop|en-US\r\n"),
            voices(&[
                ("Microsoft Huihui Desktop", "zh-CN"),
                ("Microsoft Zira Desktop", "en-US")
            ])
        );
        assert_eq!(primary_lang("zh_CN"), "zh");
    }

    #[test]
    fn default_speaker_always_returns_something() {
        // On a machine without a speech engine this is the console fallback.
        let speaker = default_speaker(&VoiceRequest::for_lang("en-US"));
        speaker.say("test");
    }
}
