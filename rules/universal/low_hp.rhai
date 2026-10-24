// Warn when health is low.
fn check(s) {
    if s.alive && s.health_percent > 0 && s.health_percent < 25 {
        #{ say: "Low health. Back off.", priority: 3, cooldown: 20 }
    }
}
