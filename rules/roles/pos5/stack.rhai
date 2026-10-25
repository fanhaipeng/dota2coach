// Stack a camp for the carry: creeps leave the camp at the minute mark, so
// pull them at about x:53. Reminds every two minutes while laning.
fn check(s) {
    let t = s.clock_time;
    if t >= 120 && t < 720 && t % 120 >= 40 && t % 120 < 43 && s.alive {
        #{ msg: "pos5.stack", priority: 1, cooldown: 110 }
    }
}
