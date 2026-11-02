// From the screen's minimap (`run --screen`): two or more enemies close to you
// after laning and none of your team, so a gank may be coming.
fn check(s) {
    if s.screen && s.alive && s.enemies_near >= 2 && s.allies_near == 0 && s.clock_time >= 600 {
        #{ msg: "screen.gank", args: #{ count: s.enemies_near }, priority: 3, cooldown: 45 }
    }
}
