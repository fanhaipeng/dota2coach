// Rune reminders, 15 seconds before each spawn.
//
// Spawn times change between patches: check them against the current patch
// notes and adjust the numbers here (no recompiling needed).
//   - Water runes: 2:00 and 4:00
//   - Power runes: every 2 minutes from 6:00
//   - Bounty runes: every 3 minutes from 3:00
//   - Wisdom runes: every 7 minutes from 7:00

fn soon(t, every, first) {
    // True for 5 seconds starting 15 seconds before a spawn.
    let lead = t + 15;
    lead >= first && lead % every < 5
}

fn check(s) {
    let t = s.clock_time;
    if t < 0 { return; }

    let runes = [];
    if (t >= 105 && t < 110) || (t >= 225 && t < 230) { runes.push("Water"); }
    if soon(t, 120, 360) { runes.push("Power"); }
    if soon(t, 180, 180) { runes.push("Bounty"); }
    if soon(t, 420, 420) { runes.push("Wisdom"); }
    if runes.is_empty() { return; }

    let names = runes[0];
    for i in 1..runes.len() {
        names += if i == runes.len() - 1 { " and " } else { ", " };
        names += runes[i];
    }
    #{ say: `${names} runes in 15 seconds.`, priority: 1, cooldown: 30 }
}
