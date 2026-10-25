// Ursa can take Roshan early once she has lifesteal and some levels.
fn check(s) {
    let lifesteal = s.items.contains("item_vladmir") || s.items.contains("item_morbid_mask")
        || s.items.contains("item_mask_of_madness") || s.items.contains("item_satanic");
    if s.alive && s.level >= 10 && lifesteal && s.clock_time >= 600 {
        #{ msg: "ursa.roshan", priority: 1, cooldown: 600 }
    }
}
