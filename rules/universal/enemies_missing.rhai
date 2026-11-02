// From the screen's minimap (`run --screen`): four or more enemies that were
// on the map a moment ago have vanished without dying and may be on their way
// to you.
fn check(s) {
    if s.screen && s.alive && s.enemies_missing >= 4 && s.clock_time >= 600 {
        #{ msg: "screen.missing", args: #{ count: s.enemies_missing }, priority: 2, cooldown: 120 }
    }
}
