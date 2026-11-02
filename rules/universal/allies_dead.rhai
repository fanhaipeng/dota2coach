// From the screen's top bar (`run --screen`): most of your team is dead, so
// don't take a fight alone.
fn check(s) {
    if s.screen && s.alive && s.allies_dead >= 3 && s.clock_time >= 300 {
        #{ msg: "screen.allies_dead", args: #{ count: s.allies_dead }, priority: 3, cooldown: 60 }
    }
}
