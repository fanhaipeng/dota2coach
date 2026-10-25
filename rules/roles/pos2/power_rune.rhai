// Mid controls the power runes: take them and use them to gank a side lane.
fn check(s) {
    let t = s.clock_time;
    if t >= 360 && t < 1200 && (t - 360) % 120 < 3 && s.alive && s.health_percent >= 50 {
        #{ msg: "pos2.power_rune", priority: 1, cooldown: 110 }
    }
}
