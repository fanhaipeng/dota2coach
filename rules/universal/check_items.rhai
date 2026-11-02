// Ask the player to click an enemy whose items haven't been read for a while
// (`run --screen`), while it's safe to look: dead, or at the fountain. The
// item panel then shows that hero's items and the coach reads them.
fn at_fountain(s) {
    // Roughly where each team's fountain is, in world units.
    let fx = if s.team == "radiant" { -7100 } else if s.team == "dire" { 7000 } else { return false; };
    let fy = if s.team == "radiant" { -6700 } else { 6500 };
    let dx = s.xpos - fx;
    let dy = s.ypos - fy;
    dx * dx + dy * dy < 1500 * 1500
}

fn check(s) {
    if s.stale_enemy == "" || s.clock_time < 300 { return; }
    if s.alive && !at_fountain(s) { return; }
    let hero = "hero." + s.stale_enemy.sub_string(14);
    #{ msg: "screen.check_items", args: #{ hero: [hero] }, priority: 1, cooldown: 90 }
}
