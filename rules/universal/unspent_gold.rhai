// Nudge to spend gold instead of carrying it around (and losing it on death).
fn check(s) {
    if s.alive && s.gold >= 2000 {
        #{ msg: "gold.unspent", args: #{ gold: s.gold }, priority: 1, cooldown: 120 }
    }
}
