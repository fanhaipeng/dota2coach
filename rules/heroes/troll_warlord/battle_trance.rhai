// Battle Trance keeps Troll alive: use it before dying, not after.
fn check(s) {
    if s.alive && s.ult_ready && s.health_percent > 0 && s.health_percent < 35 {
        #{ msg: "troll.battle_trance", priority: 3, cooldown: 30 }
    }
}
