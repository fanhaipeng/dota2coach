//! Saves a top-bar slot's portrait from a match's frames as a learned
//! picture of a hero, for heroes whose look (an arcana or persona) Valve's
//! portraits don't show. The coach also learns these by itself when a
//! panel name confirms the hero.
//!
//! cargo run -p vision --example learn_portrait -- games\frames\<match id> <slot 1-10> <hero> [assets\heroes]
//!
//! Slots 1 to 5 are Radiant, 6 to 10 Dire; `hero` is a short name like
//! `juggernaut`. One portrait is taken a minute in, while the hero is alive.

use std::path::PathBuf;

use vision::heroes::{References, slot_portrait};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [frames, slot, hero, ..] = &args[..] else {
        anyhow::bail!("usage: learn_portrait <frames dir> <slot 1-10> <hero> [heroes dir]");
    };
    let frames = PathBuf::from(frames);
    let slot: usize = slot
        .parse::<usize>()?
        .checked_sub(1)
        .filter(|s| *s < 10)
        .ok_or_else(|| anyhow::anyhow!("slot is 1 to 10"))?;
    let dir = PathBuf::from(args.get(3).map_or("assets/heroes", String::as_str));
    let mut refs = References::load(&dir)?;
    for clock in 60..600 {
        let Ok(image) = image::open(frames.join(format!("{clock:05}-topbar.jpg"))) else {
            continue;
        };
        let image = vision::topbar::at_detail(&image.into_rgb8()).into_owned();
        let bar = vision::topbar::read(&image);
        let alive = !bar
            .radiant
            .iter()
            .chain(&bar.dire)
            .nth(slot)
            .is_some_and(|s| s.dead);
        if alive {
            let saved = refs.learn(
                &dir,
                &format!("npc_dota_hero_{hero}"),
                &slot_portrait(&image, slot),
            )?;
            println!(
                "saved {} from {}:{:02}",
                saved.display(),
                clock / 60,
                clock % 60
            );
            return Ok(());
        }
    }
    anyhow::bail!(
        "the hero in slot {} was never alive in the first 10 minutes",
        slot + 1
    )
}
