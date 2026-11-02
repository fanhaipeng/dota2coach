// Counter items against the enemy lineup, read from the top bar's portraits
// (`run --screen`, after `fetch-heroes`). Says at most one suggestion at a
// time, when you are shopping (dead, or carrying 1000+ gold), and only for
// items you don't already have.
//
// Each counter lists the enemy heroes it answers (internal names without
// `npc_dota_hero_`) and the items that give an enemy the same problem, how
// many of them it takes, from when, and per role the
// message to say and the items that count as already having it. Roles not
// listed get nothing for that counter; "" is a role not detected yet.
// Hero names are in the `hero` table of the language files.

fn counters() {
    let invis = ["riki", "bounty_hunter", "clinkz", "weaver", "nyx_assassin",
        "mirana", "sand_king", "invoker", "treant"];
    let magic = ["lion", "lina", "zuus", "skywrath_mage", "leshrac", "tinker",
        "shadow_shaman", "puck", "storm_spirit", "queenofpain", "nyx_assassin",
        "obsidian_destroyer", "jakiro", "crystal_maiden", "lich", "ogre_magi",
        "invoker", "earthshaker", "enigma", "disruptor", "silencer", "pugna",
        "keeper_of_the_light", "warlock", "vengefulspirit", "witch_doctor"];
    let lockdown = ["legion_commander", "doom_bringer", "bane", "batrider",
        "beastmaster", "pudge", "lion"];
    let evasion = ["phantom_assassin", "windrunner", "brewmaster"];
    let heal = ["huskar", "alchemist", "necrolyte", "life_stealer", "dazzle",
        "oracle", "omniknight", "abaddon", "skeleton_king", "morphling",
        "witch_doctor", "chen", "undying"];
    let illusions = ["phantom_lancer", "terrorblade", "naga_siren",
        "chaos_knight", "meepo", "arc_warden"];
    let passives = ["bristleback", "phantom_assassin", "spectre", "huskar",
        "slardar", "ursa", "faceless_void", "troll_warlord", "axe",
        "tidehunter", "viper", "phantom_lancer"];
    let physical = ["phantom_assassin", "ursa", "troll_warlord", "juggernaut",
        "slark", "life_stealer", "templar_assassin", "monkey_king",
        "faceless_void", "sven", "drow_ranger", "sniper", "luna", "medusa",
        "antimage", "chaos_knight", "riki", "clinkz", "weaver",
        "skeleton_king", "terrorblade"];

    let dust = ["item_dust", "item_gem"];
    let vision = ["item_dust", "item_ward_sentry", "item_ward_dispenser", "item_gem"];
    let bkb = ["item_black_king_bar"];
    let bkb_pipe = ["item_black_king_bar", "item_pipe"];
    let glimmer = ["item_glimmer_cape", "item_pipe"];
    let linken = ["item_sphere"];
    let mkb = ["item_monkey_king_bar", "item_bloodthorn"];
    let skadi = ["item_skadi", "item_shivas_guard"];
    let vessel = ["item_spirit_vessel"];
    let cleave = ["item_mjollnir", "item_maelstrom", "item_gungir", "item_bfury",
        "item_radiance"];
    let silver = ["item_silver_edge"];
    let ghost = ["item_ghost", "item_ethereal_blade", "item_glimmer_cape"];

    // Pushed one by one: one big literal is too deep for the rule engine.
    let all = [];
    all.push(#{ heroes: invis, items: ["item_invis_sword", "item_silver_edge", "item_glimmer_cape", "item_shadow_amulet"], min: 1, from: 300, roles: #{
            "": ["counter.invis_core", dust],
            pos1: ["counter.invis_core", dust], pos2: ["counter.invis_core", dust],
            pos3: ["counter.invis_core", dust],
            pos4: ["counter.invis_support", vision], pos5: ["counter.invis_support", vision] } });
    all.push(#{ heroes: magic, items: [], min: 3, from: 600, roles: #{
            pos1: ["counter.magic_core", bkb], pos2: ["counter.magic_core", bkb],
            pos3: ["counter.magic_offlane", bkb_pipe],
            pos4: ["counter.magic_support", glimmer], pos5: ["counter.magic_support", glimmer] } });
    all.push(#{ heroes: lockdown, items: [], min: 1, from: 900, roles: #{
            pos1: ["counter.lockdown", linken], pos2: ["counter.lockdown", linken] } });
    all.push(#{ heroes: evasion, items: ["item_butterfly"], min: 1, from: 900, roles: #{
            pos1: ["counter.evasion", mkb] } });
    all.push(#{ heroes: heal, items: ["item_satanic", "item_heart"], min: 1, from: 600, roles: #{
            pos1: ["counter.heal_core", skadi], pos2: ["counter.heal_core", skadi],
            pos3: ["counter.heal_vessel", vessel], pos4: ["counter.heal_vessel", vessel] } });
    all.push(#{ heroes: illusions, items: ["item_manta"], min: 1, from: 600, roles: #{
            pos1: ["counter.illusions", cleave], pos2: ["counter.illusions", cleave] } });
    all.push(#{ heroes: passives, items: [], min: 1, from: 1200, roles: #{
            pos3: ["counter.passives", silver] } });
    all.push(#{ heroes: physical, items: ["item_desolator"], min: 2, from: 600, roles: #{
            pos4: ["counter.physical", ghost], pos5: ["counter.physical", ghost] } });
    all
}

// Enemy heroes this counter answers, as message keys like `hero.riki`: by
// who they are, or by an item they were seen carrying in the item panel.
fn matching(s, c, enemies) {
    let found = [];
    for hero in enemies {
        if c.heroes.contains(hero) || carries(s, hero, c.items) {
            found.push("hero." + hero);
        }
    }
    found
}

fn carries(s, hero, items) {
    let key = "npc_dota_hero_" + hero;
    if !(key in s.hero_items) { return false; }
    let seen = s.hero_items[key];
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
    if s.enemy_heroes.is_empty() || !(s.gold >= 1000 || !s.alive) { return; }
    let enemies = [];
    for name in s.enemy_heroes { enemies.push(name.sub_string(14)); }

    for c in counters() {
        if s.clock_time < c.from || !(s.role in c.roles) { continue; }
        let found = matching(s, c, enemies);
        let pick = c.roles[s.role];
        if found.len() < c.min || owns_any(s, pick[1]) { continue; }
        found.truncate(2);
        return #{ msg: pick[0], args: #{ heroes: found }, priority: 1, cooldown: 240 };
    }
}
