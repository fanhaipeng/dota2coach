// Nudge to spend gold instead of carrying it around (and losing it on death).
fn check(s) {
    if s.alive && s.gold >= 2000 {
        #{ say: `You have ${s.gold} gold. Go spend it.`, priority: 1, cooldown: 120 }
    }
}
