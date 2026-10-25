// Enrage cuts damage taken: use it before dying, not after.
fn check(s) {
    if s.alive && s.ult_ready && s.health_percent > 0 && s.health_percent < 35 {
        #{ msg: "ursa.enrage", priority: 3, cooldown: 30 }
    }
}
