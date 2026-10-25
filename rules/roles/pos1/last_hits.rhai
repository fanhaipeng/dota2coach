// Last hits against a typical pos1 at 5 and 10 minutes. Speaks when you are
// below 70% of the target. Tune the targets for your bracket.
fn check(s) {
    let t = s.clock_time;
    let target = if t >= 300 && t < 303 { 30 } else if t >= 600 && t < 603 { 65 } else { return; };
    if s.last_hits * 10 < target * 7 {
        #{ msg: "farm.last_hits_behind",
           args: #{ last_hits: s.last_hits, minutes: t / 60, target: target },
           priority: 1, cooldown: 200 }
    }
}
