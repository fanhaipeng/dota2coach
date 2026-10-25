# dota2coach

A voice coach for Dota 2. It listens to the game through Valve's official
[Game State Integration](https://github.com/antonpup/dota2gsi) (GSI) feed and
speaks short reminders at the right moment: rune timings, missing TP scroll,
low health, unspent gold, buyback, plus role- and hero-specific advice.

It only uses what GSI gives a player (your own hero), never reads game memory,
never injects into the game and never sends input. It gives advice; you play.

## Quick start

1. **Build** (needs [Rust](https://rustup.rs); on Windows also the Visual
   Studio Build Tools "Desktop development with C++" workload):

   ```
   cargo build --release
   ```

   Or download a build for your OS from the latest
   [CI run](../../actions) (`dota2coach-<OS>` artifact).

2. **Tell Dota where to send game state.** Save the config into Dota's GSI folder:

   ```
   dota2coach gsi-config > gamestate_integration_dota2coach.cfg
   ```

   | OS | Folder |
   |---|---|
   | Windows | `C:\Program Files (x86)\Steam\steamapps\common\dota 2 beta\game\dota\cfg\gamestate_integration\` |
   | macOS | `~/Library/Application Support/Steam/steamapps/common/dota 2 beta/game/dota/cfg/gamestate_integration/` |
   | Linux | `~/.local/share/Steam/steamapps/common/dota 2 beta/game/dota/cfg/gamestate_integration/` |

   Create the `gamestate_integration` folder if it doesn't exist, then add
   `-gamestateintegration` to Dota 2's launch options in Steam.

3. **Run the coach, then start Dota:**

   ```
   dota2coach run --role pos3 --record games/latest.jsonl
   ```

   A lobby game with bots is the easiest way to try it.

## Commands

| Command | What it does |
|---|---|
| `dota2coach run` | Listen on `127.0.0.1:3000` and coach live. The role (pos1 to pos5) is detected from your lane by 4:00, or set it with `--role pos1`; `--lang zh` coaches in Chinese, `--record FILE` saves the game for replaying, `--mute` prints instead of speaking. |
| `dota2coach replay FILE` | Coach a recorded game without Dota. `--speed 0` replays instantly, `--speed 10` at 10x. |
| `dota2coach gsi-config` | Print the GSI config file (`--port`, `--token` must match `run`). |

Set `RUST_LOG=debug` to see every rule that fires.

## Speech

Uses the speech engine the OS already has: `System.Speech` through PowerShell
on Windows, `say` on macOS, `spd-say` or `espeak-ng` on Linux. Without one,
advice is printed.

`--lang` picks the language (`en` or `zh`, see [Languages](rules/README.md#languages)),
and the coach picks an installed voice for it. `--voice "<name>"` uses a
specific voice instead. If no voice matches, the coach logs the installed
ones and uses the default voice. To add a Chinese voice:

- **Windows:** Settings > Time & language > Language & region > add
  **Chinese (Simplified, China)** with the *Text-to-speech* feature
  (Microsoft Huihui). Restart the coach afterwards.
- **macOS:** System Settings > Accessibility > Spoken Content > System Voice >
  Manage Voices > Chinese (China mainland), for example Tingting.
- **Linux:** `espeak-ng` includes Mandarin (`cmn`).

## Rules

Coaching rules are small Rhai scripts in [`rules/`](rules/README.md), loaded in
layers: universal, then the role pack (detected from where you lane), then the
hero pack (picked automatically from GSI). Edit them and re-run; no recompiling. Rune timings change between
patches, so check `rules/universal/runes.rhai` against the current patch.

## Layout

| Crate | Purpose |
|---|---|
| `crates/gsi` | GSI payload model, the local HTTP receiver, record and replay |
| `crates/coach` | `GameSnapshot`, the `Advisor` trait, the Rhai rule engine, and the `Coordinator` that handles cooldowns and pacing |
| `crates/voice` | Spoken output per OS |
| `crates/app` | The `dota2coach` binary and the pipeline tying them together |

## Roadmap

1. GSI, predefined rules and voice on Windows, macOS and Linux (this).
2. Post-game replay review with an LLM.
3. Minimap vision sidecar (prototype in Python, port to Rust; Windows first).
4. Live LLM advice triggered by game events.
5. A small trained model, compared against the rules.
