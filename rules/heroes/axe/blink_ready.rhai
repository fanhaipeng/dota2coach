// Example hero rule: Axe's power spike is Blink Dagger.
fn check(s) {
    if s.items.contains("item_blink") {
        #{ msg: "axe.blink_ready", priority: 2, cooldown: 100000 }
    }
}
