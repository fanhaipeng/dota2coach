// Dragon Lance adds range: use it to fight from behind the team.
fn check(s) {
    if s.items.contains("item_dragon_lance") {
        #{ msg: "sniper.dragon_lance", priority: 1, cooldown: 100000 }
    }
}
