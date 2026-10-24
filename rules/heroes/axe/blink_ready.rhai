// Example hero rule: Axe's power spike is Blink Dagger.
fn check(s) {
    if s.items.contains("item_blink") {
        #{ say: "Blink is done. Look for a Berserker's Call initiation.", priority: 2, cooldown: 100000 }
    }
}
