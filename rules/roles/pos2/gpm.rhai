// Farm check at 12:00: a pos2 below 450 gold per minute is falling behind.
fn check(s) {
    let t = s.clock_time;
    if t >= 12 * 60 && t < 12 * 60 + 3 && s.gpm < 450 {
        #{ msg: "farm.gpm_behind", args: #{ gpm: s.gpm }, priority: 1, cooldown: 200 }
    }
}
