// Counter items for what an enemy was just seen carrying in the item panel
// (`run --screen`: you clicked them on the top bar). Says the counter right
// after the click, whatever gold you have (the quick-buy tray buys it), and
// only for items you don't already have. Enemies read in the last 30 seconds
// count, so clicking several heroes in a row gives the most useful tip, naming
// up to 2 heroes that carry the same thing.
//
// Each entry lists enemy items and, per role, the message and the items that
// count as already having the counter. "" is a role not detected yet, coached
// like a core.

fn tips() {
    let dust = ["item_dust", "item_gem"];
    let vision = ["item_dust", "item_ward_sentry", "item_ward_dispenser", "item_gem"];
    let mkb = ["item_monkey_king_bar", "item_bloodthorn"];
    let halberd = ["item_heavens_halberd", "item_monkey_king_bar"];
    let skadi = ["item_skadi", "item_shivas_guard"];
    let vessel = ["item_spirit_vessel"];
    let cleave = ["item_mjollnir", "item_maelstrom", "item_gungir", "item_bfury"];
    let ghost = ["item_ghost", "item_ethereal_blade", "item_glimmer_cape"];
    let armor = ["item_assault", "item_shivas_guard"];
    let crimson = ["item_crimson_guard", "item_shivas_guard", "item_blade_mail"];
    let pop = ["item_force_staff", "item_hurricane_pike", "item_rod_of_atos",
        "item_gungir", "item_orchid", "item_bloodthorn"];
    let bkb = ["item_black_king_bar"];
    let glimmer = ["item_glimmer_cape", "item_pipe"];
    let force = ["item_force_staff", "item_hurricane_pike"];
    let dispel = ["item_manta", "item_black_king_bar", "item_lotus_orb"];
    let lotus = ["item_lotus_orb", "item_black_king_bar"];
    let euls = ["item_cyclone", "item_wind_waker", "item_lotus_orb"];

    let all = [];
    all.push(#{ items: ["item_invis_sword", "item_silver_edge", "item_glimmer_cape", "item_shadow_amulet"], roles: #{
            "": ["counter.item_invis_core", dust],
            pos1: ["counter.item_invis_core", dust], pos2: ["counter.item_invis_core", dust],
            pos3: ["counter.item_invis_core", dust],
            pos4: ["counter.item_invis_support", vision], pos5: ["counter.item_invis_support", vision] } });
    all.push(#{ items: ["item_butterfly"], roles: #{
            "": ["counter.item_evasion", mkb],
            pos1: ["counter.item_evasion", mkb], pos2: ["counter.item_evasion", mkb],
            pos3: ["counter.item_evasion_halberd", halberd] } });
    all.push(#{ items: ["item_satanic", "item_heart", "item_holy_locket", "item_guardian_greaves"], roles: #{
            "": ["counter.item_heal_core", skadi],
            pos1: ["counter.item_heal_core", skadi], pos2: ["counter.item_heal_core", skadi],
            pos3: ["counter.item_heal_vessel", vessel], pos4: ["counter.item_heal_vessel", vessel],
            pos5: ["counter.item_heal_vessel", vessel] } });
    all.push(#{ items: ["item_manta"], roles: #{
            "": ["counter.item_illusions", cleave],
            pos1: ["counter.item_illusions", cleave], pos2: ["counter.item_illusions", cleave] } });
    all.push(#{ items: ["item_desolator", "item_assault"], roles: #{
            "": ["counter.item_armor_core", armor],
            pos1: ["counter.item_armor_core", armor], pos2: ["counter.item_armor_core", armor],
            pos3: ["counter.item_armor_offlane", crimson],
            pos4: ["counter.item_armor_support", ghost], pos5: ["counter.item_armor_support", ghost] } });
    all.push(#{ items: ["item_dagon", "item_dagon_2", "item_dagon_3", "item_dagon_4", "item_dagon_5", "item_ethereal_blade"], roles: #{
            "": ["counter.item_burst_core", bkb],
            pos1: ["counter.item_burst_core", bkb], pos2: ["counter.item_burst_core", bkb],
            pos3: ["counter.item_burst_core", bkb],
            pos4: ["counter.item_burst_support", glimmer], pos5: ["counter.item_burst_support", glimmer] } });
    all.push(#{ items: ["item_orchid", "item_bloodthorn"], roles: #{
            "": ["counter.item_silence_core", dispel],
            pos1: ["counter.item_silence_core", dispel], pos2: ["counter.item_silence_core", dispel],
            pos3: ["counter.item_silence_offlane", lotus],
            pos4: ["counter.item_silence_support", euls], pos5: ["counter.item_silence_support", euls] } });
    all.push(#{ items: ["item_sphere"], roles: #{
            "": ["counter.item_linken", pop],
            pos1: ["counter.item_linken", pop], pos2: ["counter.item_linken", pop],
            pos3: ["counter.item_linken", pop], pos4: ["counter.item_linken", pop],
            pos5: ["counter.item_linken", pop] } });
    all.push(#{ items: ["item_black_king_bar"], roles: #{
            pos4: ["counter.item_bkb_support", force], pos5: ["counter.item_bkb_support", force] } });
    all
}

// Enemies whose items were read in the last 30 seconds.
fn fresh(s) {
    let found = [];
    for hero in s.enemy_heroes {
        if just_read(s, hero) { found.push(hero); }
    }
    found
}

fn just_read(s, hero) {
    if !(hero in s.hero_items_age) || !(hero in s.hero_items) { return false; }
    let age = s.hero_items_age[hero];
    age >= 0 && age <= 30
}

fn carrying(s, heroes, items) {
    let found = [];
    for hero in heroes {
        if has_any(s.hero_items[hero], items) { found.push("hero." + hero.sub_string(14)); }
    }
    found
}

fn has_any(seen, items) {
    for item in items {
        if seen.contains(item) { return true; }
    }
    false
}

fn owns_any(s, items) {
    for item in items {
        if s.items.contains(item) || s.stash.contains(item) { return true; }
    }
    false
}

fn check(s) {
    if !s.screen || s.clock_time < 300 { return; }
    let heroes = fresh(s);
    if heroes.is_empty() { return; }
    for t in tips() {
        if !(s.role in t.roles) { continue; }
        let pick = t.roles[s.role];
        if owns_any(s, pick[1]) { continue; }
        let found = carrying(s, heroes, t.items);
        if found.is_empty() { continue; }
        found.truncate(2);
        return #{ msg: pick[0], args: #{ heroes: found }, priority: 2, cooldown: 90 };
    }
}
