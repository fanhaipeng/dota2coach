// Offlaners lose the lane by dying, not by missing creeps.
fn check(s) {
    if s.since_death >= 0 && s.since_death < 3 && s.deaths >= 2 && s.clock_time < 480 {
        #{ msg: "pos3.early_deaths", priority: 2, cooldown: 300 }
    }
}
