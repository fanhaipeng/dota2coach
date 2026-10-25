// Supports should keep vision up: remind when no wards are carried.
fn check(s) {
    let wards = s.items.contains("item_ward_observer") || s.items.contains("item_ward_dispenser");
    if s.clock_time >= 120 && s.alive && !wards {
        #{ msg: "support.wards", priority: 1, cooldown: 240 }
    }
}
