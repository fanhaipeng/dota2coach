// Remind to carry a TP scroll once the laning stage is under way: only when
// you can afford one, and at most every 4 minutes. Boots of Travel, or a TP
// waiting in the stash, count as having one.
fn has_tp(s) {
    for item in ["item_tpscroll", "item_travel_boots", "item_travel_boots_2"] {
        if s.items.contains(item) || s.stash.contains(item) { return true; }
    }
    false
}

fn check(s) {
    if s.clock_time > 120 && s.alive && s.gold >= 100 && !has_tp(s) {
        #{ msg: "no_tp", priority: 2, cooldown: 240 }
    }
}
