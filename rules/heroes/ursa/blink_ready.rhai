// Blink lets Ursa jump on supports before they can react.
fn check(s) {
    if s.items.contains("item_blink") {
        #{ msg: "ursa.blink_ready", priority: 2, cooldown: 100000 }
    }
}
