// Dying with a lot of gold loses part of it. Say so right after the death.
fn check(s) {
    if s.since_death >= 0 && s.since_death < 3 && s.gold_at_death >= 1500 {
        #{ msg: "death.gold", args: #{ gold: s.gold_at_death }, priority: 2, cooldown: 20 }
    }
}
