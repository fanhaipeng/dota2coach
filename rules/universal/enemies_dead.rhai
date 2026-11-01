// From the screen's top bar (`run --screen`): enemies dead long enough to take
// an objective. Three or more after 15 minutes is a Roshan or high ground
// chance; otherwise push a tower.
fn check(s) {
    if s.screen && s.alive && s.enemies_dead_long >= 2 {
        let args = #{ count: s.enemies_dead_long, secs: s.dead_window };
        if s.enemies_dead_long >= 3 && s.clock_time >= 900 {
            #{ msg: "screen.roshan", args: args, priority: 3, cooldown: 40 }
        } else {
            #{ msg: "screen.push", args: args, priority: 3, cooldown: 40 }
        }
    }
}
