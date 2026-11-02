//! Reads the hero name in the item panel of a match's saved frames with
//! Windows text recognition and checks it against your hero from GSI (your
//! own panel shows your name).
//!
//! cargo run -p dota2coach --example panel_names -- games\frames\<match id> [--every SECS]

use std::path::PathBuf;

use dota2coach::HeroNames;
use vision::heroes::{LineupReader, References};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let frames = PathBuf::from(&args[0]);
    let every: usize = args
        .iter()
        .position(|a| a == "--every")
        .and_then(|i| args.get(i + 1)?.parse().ok())
        .unwrap_or(10);
    let match_id = frames.file_name().unwrap().to_string_lossy().into_owned();
    let recording = std::fs::read_dir(frames.parent().unwrap().parent().unwrap())?
        .filter_map(|e| Some(e.ok()?.path()))
        .find(|p| p.to_string_lossy().ends_with(&format!("-{match_id}.jsonl")))
        .expect("no recording");
    let you = std::fs::read_to_string(&recording)?
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find_map(|l| {
            let name = l["payload"]["hero"]["name"].as_str()?;
            (!name.is_empty()).then(|| name.to_owned())
        })
        .expect("no hero in the recording");

    // The lineup, read from the first minutes as live.
    let refs = References::load(&PathBuf::from("assets/heroes"))?;
    let mut lineup = LineupReader::new(refs);
    for clock in 0..300 {
        let Ok(image) = image::open(frames.join(format!("{clock:05}-topbar.jpg"))) else {
            continue;
        };
        let image = normalise(image.into_rgb8());
        lineup.observe(&image, &vision::topbar::read(&image));
    }
    let lineup = lineup.lineup().expect("lineup not read");
    let heroes: Vec<String> = lineup.radiant.iter().chain(&lineup.dire).cloned().collect();

    let names = HeroNames::load(&dota2coach::resolve_rules_dir(None))?;
    let languages = vision::ocr::languages();
    println!("you: {you}; text recognition languages: {languages:?}");
    let (mut n, mut right, mut unsure) = (0, 0, 0);
    for clock in (0..3100).step_by(every) {
        let Ok(image) = image::open(frames.join(format!("{clock:05}-items.jpg"))) else {
            continue;
        };
        let pictures = vision::items::name_pictures(&image.into_rgb8());
        let mut texts = Vec::new();
        for lang in &languages {
            for picture in &pictures {
                if let Ok(text) = vision::ocr::read_text(picture, lang)
                    && !text.trim().is_empty()
                {
                    texts.push(text);
                }
            }
        }
        n += 1;
        match names.identify(&texts, &heroes) {
            Some(hero) if hero == you => right += 1,
            Some(hero) => println!(
                "  {}:{:02} read {hero} from {texts:?}",
                clock / 60,
                clock % 60
            ),
            None => {
                unsure += 1;
                if unsure <= 5 {
                    println!("  {}:{:02} unsure from {texts:?}", clock / 60, clock % 60);
                }
            }
        }
    }
    println!(
        "your name read right in {right} of {n} frames ({:.1}%), unsure in {unsure}, wrong in {}",
        right as f64 * 100.0 / n.max(1) as f64,
        n - right - unsure
    );
    Ok(())
}

/// Frames saved before the crops were tightened hold the top bar at full
/// detail inside a wider band (1.3 by 0.09 of the screen height). Cut the
/// current crop out of those and scale it to 1080p detail.
fn normalise(image: image::RgbImage) -> image::RgbImage {
    let (w, h) = image.dimensions();
    if w * 100 / h <= 1350 {
        return vision::topbar::at_detail(&image).into_owned();
    }
    let screen = w * 10 / 13;
    let area = vision::top_bar_area(screen * 16 / 9, screen);
    let x = (w - area.width) / 2;
    let part = image::imageops::crop_imm(&image, x, 0, area.width, area.height).to_image();
    image::imageops::resize(
        &part,
        vision::topbar::WIDTH,
        vision::topbar::HEIGHT,
        image::imageops::FilterType::CatmullRom,
    )
}
