// As the carry, start thinking about farm patterns once laning ends.
fn check(s) {
    if s.clock_time >= 600 && s.clock_time < 605 {
        #{ msg: "pos1.farm_route", priority: 1, cooldown: 600 }
    }
}
