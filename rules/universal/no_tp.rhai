// Remind to carry a TP scroll once the laning stage is under way.
fn check(s) {
    if s.clock_time > 120 && s.alive && !s.items.contains("item_tpscroll") {
        #{ say: "You have no TP scroll.", priority: 2, cooldown: 90 }
    }
}
