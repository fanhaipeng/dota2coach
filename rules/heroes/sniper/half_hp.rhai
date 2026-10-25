// Sniper dies fast: back off earlier than the universal low-health warning.
fn check(s) {
    if s.alive && s.health_percent > 0 && s.health_percent < 50 {
        #{ msg: "sniper.half_hp", priority: 2, cooldown: 45 }
    }
}
