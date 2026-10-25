// As the offlaner, look to make space once laning ends.
fn check(s) {
    if s.clock_time >= 600 && s.clock_time < 605 {
        #{ msg: "pos3.pressure", priority: 1, cooldown: 600 }
    }
}
