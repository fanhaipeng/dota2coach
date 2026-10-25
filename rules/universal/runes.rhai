// Rune and shrine reminders, 15 seconds before each spawn.
//
// Spawn times change between patches. Checked for 7.41 (October 2026):
//   - Water runes: 2:00 and 4:00
//   - Power runes: every 2 minutes from 6:00
//   - Bounty runes: every 4 minutes (0:00, then 4:00, 8:00, ...)
//   - Wisdom shrines: every 7 minutes from 7:00 (they replaced wisdom runes)
// Adjust the numbers here when a patch changes them; no recompiling needed.

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
    if (t >= 105 && t < 110) || (t >= 225 && t < 230) { runes.push("rune.water"); }
    if soon(t, 120, 360) { runes.push("rune.power"); }
    if soon(t, 240, 240) { runes.push("rune.bounty"); }

    let out = [];
    if !runes.is_empty() {
        out.push(#{ msg: "runes.soon", args: #{ runes: runes }, priority: 1, cooldown: 30 });
    }
    if soon(t, 420, 420) {
        out.push(#{ msg: "wisdom.soon", priority: 1, cooldown: 30 });
    }
    out
}
