// Zeus is all spells: without mana he is useless in a fight.
fn check(s) {
    if s.alive && s.mana_percent < 20 && s.clock_time > 0 {
        #{ msg: "zeus.low_mana", priority: 1, cooldown: 90 }
    }
}
