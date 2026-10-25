// Corrosive Haze reveals and shreds armor: remind once when it is learned.
fn check(s) {
    if s.ult_level >= 1 && s.ult_ready {
        #{ msg: "slardar.haze_ready", priority: 1, cooldown: 100000 }
    }
}
