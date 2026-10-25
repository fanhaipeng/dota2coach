// Assassinate finishes heroes who escape at low health.
fn check(s) {
    if s.alive && s.ult_ready {
        #{ msg: "sniper.assassinate_ready", priority: 1, cooldown: 240 }
    }
}
