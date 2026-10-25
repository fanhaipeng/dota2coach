// Viper Strike is a strong kill threat in lane and early fights.
fn check(s) {
    if s.alive && s.ult_ready && s.clock_time < 1500 {
        #{ msg: "viper.strike_ready", priority: 1, cooldown: 240 }
    }
}
