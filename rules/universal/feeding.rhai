// Many early deaths: slow down until items come.
fn check(s) {
    if s.since_death >= 0 && s.since_death < 3 && s.deaths >= 4 && s.clock_time < 900 {
        #{ msg: "death.many", args: #{ deaths: s.deaths }, priority: 2, cooldown: 240 }
    }
}
