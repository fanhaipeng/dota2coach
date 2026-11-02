// Nudge to spend gold instead of carrying it around (and losing it on death).
// Quiet while an item is queued in the quick-buy tray: it buys itself once
// the gold is there.
fn check(s) {
    if s.alive && s.gold >= 2000 && !s.quickbuy_set {
        #{ msg: "gold.unspent", args: #{ gold: s.gold }, priority: 1, cooldown: 120 }
    }
}
