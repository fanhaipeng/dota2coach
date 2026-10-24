# Coaching rules

Rules are small [Rhai](https://rhai.rs) scripts, loaded in layers each game:

| Folder | When it loads |
|---|---|
| `universal/` | Every game |
| `roles/<role>/` | When you pass `--role <role>`, for example `--role pos3` |
| `heroes/<hero>/` | Automatically, once GSI reports your hero (folder name without `npc_dota_hero_`, for example `axe`) |

Each file defines `fn check(s)`. It returns nothing when there's nothing to
say, or a map:

```rhai
fn check(s) {
    if s.clock_time > 120 && !s.items.contains("item_tpscroll") {
        #{ say: "You have no TP scroll.", priority: 2, cooldown: 90 }
    }
}
```

- `say`: the sentence to speak.
- `priority`: 1 reminder, 2 act soon, 3 act now. The most urgent message wins. Default 1.
- `cooldown`: game seconds before this rule can speak again. Default 60.

A rule can also return an array of maps. The coach speaks at most one message
every few seconds (`--gap`) and stays quiet while the game is paused.

## Fields on `s`

`match_id`, `game_state`, `in_game`, `paused`, `clock_time`, `daytime`,
`hero`, `level`, `alive`, `respawn_seconds`, `health_percent`,
`mana_percent`, `buyback_cost`, `buyback_cooldown`, `smoked`, `gold`,
`kills`, `deaths`, `assists`, `last_hits`, `denies`, `items` (array of item
names you carry, including the TP and neutral slots), `stash`.

See `crates/coach/src/snapshot.rs` for the full definition.

## Trying a rule

Record a game with `dota2coach run --record game.jsonl`, then replay it as fast
as possible while you tune rules:

```
dota2coach replay game.jsonl --speed 0 --mute
```
