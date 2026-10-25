// Viper wins lanes by trading: Poison Attack costs little to use.
fn check(s) {
    if s.clock_time >= 20 && s.clock_time < 25 {
        #{ msg: "viper.harass", priority: 1, cooldown: 100000 }
    }
}
