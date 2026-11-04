// From the screen's minimap (`run --screen`): Roshan's skull has gone from his
// pit, so he died. Say when he's back (8 to 11 minutes later), then count down:
// a minute before he can be back, when he can, and a minute before he surely is.
fn clock(t) {
    let secs = t % 60;
    if secs < 10 { `${t / 60}:0${secs}` } else { `${t / 60}:${secs}` }
}

fn check(s) {
    if s.screen && s.roshan_dead {
        let since = s.clock_time - s.roshan_killed_at;
        let args = #{ earliest: clock(s.roshan_killed_at + 480), latest: clock(s.roshan_killed_at + 660) };
        // Each call has its own 30-second window, at least a minute apart.
        if since >= 60 && since < 90 {
            #{ msg: "roshan.killed", args: args, priority: 2, cooldown: 50 }
        } else if since >= 420 && since < 450 {
            #{ msg: "roshan.soon", args: args, priority: 3, cooldown: 50 }
        } else if since >= 480 && since < 510 {
            #{ msg: "roshan.window", args: args, priority: 3, cooldown: 50 }
        } else if since >= 600 && since < 630 {
            #{ msg: "roshan.last", args: args, priority: 3, cooldown: 50 }
        }
    }
}
