// From 30 minutes on, warn when you can't afford buyback.
fn check(s) {
    if s.clock_time >= 1800 && s.alive && s.buyback_cooldown == 0 && s.gold < s.buyback_cost {
        #{ say: "No buyback gold. Play safe.", priority: 2, cooldown: 300 }
    }
}
