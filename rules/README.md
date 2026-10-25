# Coaching rules

Rules are small [Rhai](https://rhai.rs) scripts, loaded in layers each game:

| Folder | When it loads |
|---|---|
| `universal/` | Every game |
| `roles/<role>/` | Once your position is detected at about 4:00, from the lane you play in (`pos1` to `pos5`). `--role pos3` sets it instead. |
| `heroes/<hero>/` | Automatically, once GSI reports your hero (folder name without `npc_dota_hero_`, for example `axe`) |

Role packs exist for `pos1` to `pos5`: last-hit and GPM checks against
typical targets for cores (tune the numbers in `last_hits.rhai` and `gpm.rhai`
for your bracket), rune control for mid, wards, rotations and stacking for
supports.

Hero folders use Dota's internal names, which don't always match the hero's
name: Zeus is `zuus`, Troll Warlord is `troll_warlord`. Packs so far: `axe`
(example), `slardar`, `sniper`, `troll_warlord`, `ursa`, `viper`, `zuus`.
Prefer `s.ult_ready` and item names over ability names in hero rules, since
ability names change between patches more often.

Each file defines `fn check(s)`. It returns nothing when there's nothing to
say, or a map:

```rhai
fn check(s) {
    if s.clock_time > 120 && !s.items.contains("item_tpscroll") {
        #{ msg: "no_tp", priority: 2, cooldown: 90 }
    }
}
```

- `msg`: a message key; the words come from the language files (below).
- `args`: values for the message's `{placeholders}`, for example
  `args: #{ gold: s.gold }`. An array is a list of message keys, each
  translated and joined the way the language lists things ("Power and
  Bounty", "能量符和赏金符").
- `say`: instead of `msg`, a literal sentence spoken as it is in every
  language. Handy while drafting a rule.
- `priority`: 1 reminder, 2 act soon, 3 act now. The most urgent message wins. Default 1.
- `cooldown`: game seconds before this rule can speak again. Default 60.

A rule can also return an array of maps. The coach speaks at most one message
every few seconds (`--gap`) and stays quiet while the game is paused.

## Languages

Run with `--lang en` (default) or `--lang zh`. The words for each message key
live in TOML files named after the language, next to the rules that use them:

| File | Holds |
|---|---|
| `lang/<code>.toml` | Messages of the universal rules |
| `roles/<role>/lang/<code>.toml` | Messages of that role pack |
| `heroes/<hero>/lang/<code>.toml` | Messages of that hero pack |

```toml
no_tp = "You have no TP scroll."     # key no_tp

[_meta]
name = "English"
voice = "en-US"          # which installed voice to speak with
list_separator = ", "
list_last = " and "

[gold]
unspent = "You have {gold} gold. Go spend it."   # key gold.unspent
```

Tables become dotted keys, and top-level keys must come before the first
table. Prefix pack messages with the pack name (`axe.blink_ready`) so they
don't clash.

**Adding a language** is adding files, no code: copy every `en.toml` to
`<code>.toml`, translate the values and set `[_meta]`. A missing message
falls back to English, and the coach logs which ones at startup. The test
suite checks that every shipped language has every message.

## Fields on `s`

`match_id`, `game_state`, `in_game`, `paused`, `clock_time`, `daytime`,
`team` (`radiant` or `dire`), `role` (`pos1` to `pos5`, empty until known),
`hero`, `xpos`, `ypos`, `level`, `alive`, `respawn_seconds`, `health_percent`,
`mana_percent`, `buyback_cost`, `buyback_cooldown`, `smoked`, `ult_level`,
`ult_ready` (ultimate learned, off cooldown and castable), `gold`,
`kills`, `deaths`, `assists`, `last_hits`, `denies`, `gpm`, `xpm`,
`since_death` (game seconds since you last died, -1 if you haven't),
`gold_at_death` (gold you carried into your last death), `items` (array of item
names you carry, including the TP and neutral slots), `stash`.

See `crates/coach/src/snapshot.rs` for the full definition.

## Trying a rule

Record a game with `dota2coach run --record game.jsonl`, then replay it as fast
as possible while you tune rules:

```
dota2coach replay game.jsonl --speed 0 --mute
```
