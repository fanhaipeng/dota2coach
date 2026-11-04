//! Lists the Roshan kills the coach would have seen in a match's saved
//! minimap frames, and when it would have said he's back.
//!
//! cargo run -p dota2coach --example roshan -- games\frames\<match id>

use std::path::PathBuf;

use coach::GameSnapshot;
use dota2coach::{RoshanTimer, format_clock};

fn main() -> anyhow::Result<()> {
    let frames = PathBuf::from(std::env::args().nth(1).expect("usage: roshan <frames dir>"));
    let mut clocks: Vec<i64> = std::fs::read_dir(&frames)?
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            name.strip_suffix("-minimap.jpg")?.parse().ok()
        })
        .collect();
    clocks.sort();
    let mut timer = RoshanTimer::default();
    let mut dead = false;
    for clock in clocks {
        let image = image::open(frames.join(format!("{clock:05}-minimap.jpg")))?.into_rgb8();
        let mut s = GameSnapshot {
            match_id: "1".into(),
            clock_time: clock,
            ..Default::default()
        };
        timer.observe(&mut s, vision::minimap::roshan_shown(&image));
        if s.roshan_dead && !dead {
            let at = s.roshan_killed_at;
            println!(
                "Roshan killed at {} (seen at {}); back between {} and {}",
                format_clock(at),
                format_clock(clock),
                format_clock(at + 480),
                format_clock(at + 660)
            );
        } else if !s.roshan_dead && dead {
            println!("  back on the minimap at {}", format_clock(clock));
        }
        dead = s.roshan_dead;
    }
    Ok(())
}
