# dota2coach

A voice coach for Dota 2. While you play, it speaks short reminders at the
right moment: rune timings, a missing TP scroll, low health, unspent gold,
buyback, plus advice for your role and hero. It speaks English or Chinese.

It only reads Valve's official
[Game State Integration](https://github.com/antonpup/dota2gsi)
(GSI) feed, which Dota sends to programs you allow, and only about your own
hero. It never reads game memory, never injects into the game and never sends
input, so it is as safe to use as any stats overlay. It gives advice; you play.

## Getting started

Setup takes about five minutes and is the same on Windows, macOS and Linux.

### 1. Get the coach

**Download** the zip for your OS from the [Releases](../../releases) page
(`dota2coach-Windows-X64.zip`, `dota2coach-macOS-ARM64.zip` or
`dota2coach-Linux-X64.zip`) and unzip it anywhere, for example
`Documents\dota2coach`. Keep the `rules` folder next to the program.

On macOS, allow the downloaded program to run once from Terminal, in the
unzipped folder:

```
chmod +x dota2coach && xattr -c dota2coach
```

Or **build it yourself** (see [Building from source](#building-from-source)).

The commands below are typed in a terminal opened in the coach's folder:
PowerShell on Windows (in File Explorer, Shift + right-click the folder >
*Open in Terminal*), Terminal on macOS and Linux. On Windows write
`.\dota2coach.exe`, on macOS and Linux `./dota2coach`.

### 2. Connect it to Dota

```
.\dota2coach.exe setup
```

This finds Dota 2 in any of your Steam libraries (including other drives such
as `F:\SteamLibrary`) and installs a small config file that tells Dota to send
game state to the coach. If it can't find Dota, open Steam > right-click
Dota 2 > *Properties* > *Installed Files* > *Browse*, copy that folder's path
and run `.\dota2coach.exe setup --dota-dir "<that path>"`.

Then, once, in Steam: right-click Dota 2 > *Properties* > *General* >
*Launch Options*, and add `-gamestateintegration` (after a space if something
is already there). Restart Dota if it was running: it only reads the config
at startup.

### 3. Play

Start the coach first, then Dota:

```
.\dota2coach.exe run
```

Or in Chinese:

```
.\dota2coach.exe run --lang zh
```

Start a game (a lobby with bots is the easiest way to try it). Once you have a
hero, the coach window prints `new match` and loads your hero's advice. At
about 4:00 it tells you which role it thinks you are playing, from the lane
you are in, and switches to that role's advice. Press Ctrl+C to stop it.

Every match is recorded to the `games` folder, so you can review it.

### 4. Review the game

After the game:

```
.\dota2coach.exe review
```

It reviews your latest game: your farm against the target for your role, each
death (where, how much gold you lost, whether you had a TP and what the coach
had warned you about just before), your item timings, how long you sat on
unspent gold, and what the coach told you most often. Add `--lang zh` for
Chinese, or pass a file from `games` to review an older game.

For a coach's write-up on top of the facts (your costliest mistakes, one
thing you did well and one thing to focus on next game), add `--coach`. Claude
writes it, so it needs your own Anthropic API key, created in the
[Claude Console](https://platform.claude.com). Set it once, then open a new
terminal:

```
setx ANTHROPIC_API_KEY "sk-ant-..."
.\dota2coach.exe review --coach --lang zh
```

On macOS or Linux, `export ANTHROPIC_API_KEY=sk-ant-...` instead. Only the
facts of the game are sent, not the recording. A review costs a few cents of
API usage.

If Windows Firewall asks about the coach, allowing private networks is enough;
it only listens on your own computer (`127.0.0.1`).

## Options

| Command | What it does |
|---|---|
| `run` | Coach live. Options: `--lang zh` for Chinese; `--role pos3` to set your role instead of detecting it (`pos1` to `pos5`); `--voice "<name>"` to pick a voice; `--mute` to print advice instead of speaking. Each match is saved to `games/` for `review`; `--games DIR` changes the folder, `--record FILE` saves everything to one file and `--no-record` turns it off. |
| `review [FILE]` | Review your latest game, or the recording given. `--lang zh` in Chinese, `--json` for the raw facts, `--role pos3` if the role was detected wrong, `--coach` for Claude's write-up (needs `ANTHROPIC_API_KEY`; `--model` picks the Claude model). |
| `replay FILE` | Coach a recorded game without Dota. `--speed 0` replays instantly, `--speed 5` at five times speed. |
| `setup` | Find Dota and install the GSI config. `--dota-dir` if it isn't found. |
| `gsi-config` | Print the GSI config instead of installing it. |

Set `RUST_LOG=debug` to see every rule that fires.

## Voices and languages

The coach uses the speech your OS already has, and picks an installed voice
for the language: `System.Speech` on Windows, `say` on macOS, `spd-say` or
`espeak-ng` on Linux. Without one, advice is printed.

For Chinese you need a Chinese voice:

- **Windows:** Settings > Time & language > Language & region > *Add a
  language* > **Chinese (Simplified, China)**, with *Text-to-speech* ticked
  (you can untick the display language). Then restart the coach. It should
  log `using voice voice="Microsoft Huihui Desktop"`.
- **macOS:** System Settings > Accessibility > Spoken Content > System Voice >
  Manage Voices > Chinese (China mainland), for example Tingting.
- **Linux:** `espeak-ng` includes Mandarin.

If no voice matches, the coach logs the installed voices; pass one with
`--voice "<name>"`. More languages can be added without code: see
[Languages](rules/README.md#languages).

## Troubleshooting

**The coach never prints `new match`.**
1. Check the config is installed: run `setup` again and look at the path it
   prints. It must be inside the same Dota install Steam launches.
2. Check the launch option `-gamestateintegration` is set.
3. Fully quit and restart Dota after installing the config.
4. Start the coach before entering a game, and keep its window open.
5. If you wrote the config yourself with PowerShell's `>`, it was saved as
   UTF-16, which Dota ignores. Use `setup` instead.

**"Failed to start process for this game: OS Error 0" (macOS).** This is a
Steam problem, not the coach's. Clear the launch options to confirm, then try
turning off Steam Cloud for Dota, verifying the game files, reinstalling
Rosetta (`softwareupdate --install-rosetta --agree-to-license`) or
reinstalling Steam.

**It speaks English words in Chinese mode, or nothing at all.** No voice for
the language is installed: see [Voices and languages](#voices-and-languages).

**It said the wrong role.** Pass `--role` (for example `--role pos3`).
Detection looks at your lane between 0:30 and 4:00, so rotating a lot early
can confuse it.

## Building from source

Install [Rust](https://rustup.rs) and Git. On Windows also install the Visual
Studio Build Tools with the "Desktop development with C++" workload:

```
winget install Git.Git
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

(If winget reports Build Tools are already installed but fails to upgrade,
add the C++ workload from the Visual Studio Installer instead.)

Then:

```
git clone https://github.com/fanhaipeng/dota2coach.git
cd dota2coach
cargo test
cargo build --release
```

The program is `target/release/dota2coach` (`.exe` on Windows); run it from
the repository folder so it finds `rules/`. Try it without Dota on the sample
game: `dota2coach replay fixtures/sample_game.jsonl --speed 5`.

Publishing a release: push a tag such as `v0.1.0`. CI builds every OS and
attaches the zips to a GitHub Release.

## Rules

Coaching rules are small Rhai scripts in [`rules/`](rules/README.md), loaded in
layers: universal, then the role pack (detected from where you lane), then the
hero pack (picked automatically from GSI). Edit them and re-run; no
recompiling. Anyone can add a hero or role pack, or a language, by adding a
folder or file: see [rules/README.md](rules/README.md). Rune timings change
between patches, so check `rules/universal/runes.rhai` against the current
patch.

## Layout

| Crate | Purpose |
|---|---|
| `crates/gsi` | GSI payload model, the local HTTP receiver, record and replay |
| `crates/coach` | `GameSnapshot`, the `Advisor` trait, the Rhai rule engine, and the `Coordinator` that handles cooldowns and pacing |
| `crates/voice` | Spoken output per OS |
| `crates/app` | The `dota2coach` binary and the pipeline tying them together |

## Roadmap

1. GSI, predefined rules and voice on Windows, macOS and Linux (done).
2. Post-game review: the facts (`review`, done), a coach's write-up by Claude (`review --coach`, done), then trends across games.
3. Minimap vision sidecar (prototype in Python, port to Rust; Windows first).
4. Live LLM advice triggered by game events.
5. A small trained model, compared against the rules.
