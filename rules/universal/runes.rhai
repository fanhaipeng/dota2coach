// Rune and shrine reminders, 15 seconds before each spawn.
//
// Spawn times change between patches. Checked for 7.41 (October 2026):
//   - Water runes: 2:00 and 4:00
//   - Power runes: every 2 minutes from 6:00
//   - Bounty runes: every 4 minutes (0:00, then 4:00, 8:00, ...)
//   - Wisdom shrines: every 7 minutes from 7:00 (they replaced wisdom runes)
//   - Healing lotus: every 3 minutes from 3:00, at the two lotus pools
// Adjust the numbers here when a patch changes them; no recompiling needed.
//
// Only what you can use is said: water and power runes when you're near the
// river spots they spawn at, the healing lotus when you're near its pool, bounty runes and wisdom shrines to supports (who
// take them).

// Where water and power runes spawn: the top and bottom river spots, in world
// units (approximate). Within 3500 units counts as near.
fn near(s, x, y, within) {
    let dx = s.xpos - x;
    let dy = s.ypos - y;
    dx * dx + dy * dy < within * within
}

fn near_river_rune(s) {
    near(s, -1760, 1216, 3500) || near(s, 1168, -1200, 3500)
}

// The lotus pools near the top-left and bottom-right corners, measured on the
// minimap (about 200 units off at most). Within 2500 units, about a screen
// and a half, counts as near.
fn near_lotus_pool(s) {
    near(s, -7530, 4060, 2500) || near(s, 7530, -4550, 2500)
}

fn takes_side_runes(s) {
    s.role == "pos4" || s.role == "pos5"
}

fn soon(t, every, first) {
    // True for 5 seconds starting 15 seconds before a spawn.
    let lead = t + 15;
    lead >= first && (lead - first) % every < 5
}

fn check(s) {
    let t = s.clock_time;
    if t < 0 { return; }

    // Message keys; the language's catalog names them and joins the list.
    let runes = [];
    let river = near_river_rune(s);
    if river && (t >= 105 && t < 110) || (t >= 225 && t < 230) { runes.push("rune.water"); }
    if river && soon(t, 120, 360) { runes.push("rune.power"); }
    if takes_side_runes(s) && soon(t, 240, 240) { runes.push("rune.bounty"); }

    let out = [];
    if !runes.is_empty() {
        out.push(#{ msg: "runes.soon", args: #{ runes: runes }, priority: 1, cooldown: 30 });
    }
    if near_lotus_pool(s) && soon(t, 180, 180) {
        out.push(#{ msg: "lotus.soon", priority: 1, cooldown: 30 });
    }
    if takes_side_runes(s) && soon(t, 420, 420) {
        out.push(#{ msg: "wisdom.soon", priority: 1, cooldown: 30 });
    }
    out
}
