// Farm check at 15:00: a pos1 below 450 gold per minute is falling behind.
fn check(s) {
    let t = s.clock_time;
    if t >= 15 * 60 && t < 15 * 60 + 3 && s.gpm < 450 {
        #{ msg: "farm.gpm_behind", args: #{ gpm: s.gpm }, priority: 1, cooldown: 200 }
    }
}
