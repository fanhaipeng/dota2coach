// Thundergod's Wrath hits every enemy hero: watch the map for kills.
fn check(s) {
    if s.alive && s.ult_ready {
        #{ msg: "zeus.wrath_ready", priority: 1, cooldown: 180 }
    }
}
