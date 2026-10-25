// Blink turns Slardar into an initiator.
fn check(s) {
    if s.items.contains("item_blink") {
        #{ msg: "slardar.blink_ready", priority: 2, cooldown: 100000 }
    }
}
