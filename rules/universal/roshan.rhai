// From the screen's minimap (`run --screen`): Roshan's skull has gone from his
// pit, so he died. Say when he's back (8 to 11 minutes later), and remind
// shortly before the earliest respawn.
fn clock(t) {
    let secs = t % 60;
    if secs < 10 { `${t / 60}:0${secs}` } else { `${t / 60}:${secs}` }
}

fn check(s) {
    if s.screen && s.roshan_dead {
        let since = s.clock_time - s.roshan_killed_at;
        let args = #{ earliest: clock(s.roshan_killed_at + 480), latest: clock(s.roshan_killed_at + 660) };
        if since >= 60 && since < 90 {
            #{ msg: "roshan.killed", args: args, priority: 2, cooldown: 300 }
        } else if since >= 450 && since < 480 {
            #{ msg: "roshan.soon", args: args, priority: 3, cooldown: 300 }
        }
    }
}
