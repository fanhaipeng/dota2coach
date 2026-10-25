// Soft supports win games by moving: look for a rotation after the lane.
fn check(s) {
    let t = s.clock_time;
    if (t >= 300 && t < 303) || (t >= 540 && t < 543) {
        if s.alive && s.health_percent >= 60 {
            #{ msg: "pos4.rotate", priority: 1, cooldown: 200 }
        }
    }
}
