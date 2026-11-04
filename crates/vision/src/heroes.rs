//! Reads who is playing from the top bar: matches each hero portrait against
//! Valve's hero portraits, kept in a local folder (`dota2coach fetch-heroes`).
//!
//! One frame can be fooled by a death animation or something drawn over the
//! bar, so [`LineupReader`] votes over many frames and only answers once each
//! slot has a clear winner.

use std::path::{Path, PathBuf};

use image::RgbImage;

use crate::gallery::{Gallery, ThumbSize};
use crate::topbar::{self, TopBar};

/// The portrait inside a top-bar slot (at 1080p detail): below the player
/// colour strip, and short of the gap to the next portrait.
const PORTRAIT_WIDTH: u32 = 60;
const PORTRAIT_TOP: u32 = 4;
const PORTRAIT_HEIGHT: u32 = 36;
/// Portraits are compared by the top of their thumbnails, above the marks
/// drawn on your own hero's portrait.
const THUMB: ThumbSize = ThumbSize {
    w: 24,
    h: 14,
    rows: 10,
};
/// The hero's face in the item panel (at 1080p detail), left of the stats,
/// compared as a thumbnail with the middle of each portrait.
const FACE_X: u32 = 112;
const FACE_Y: u32 = 60;
const FACE_W: u32 = 104;
const FACE_H: u32 = 110;
const FACE: ThumbSize = ThumbSize {
    w: 12,
    h: 12,
    rows: 12,
};
/// The folder, inside the hero portraits folder, of portraits learned from
/// matches.
const LEARNED: &str = "learned";
/// Votes a slot needs, and the share of them its winner needs, before the
/// lineup counts as read.
const MIN_VOTES: u32 = 20;
const MIN_SHARE: f32 = 0.6;
/// A slot whose best match is at least this close to its runner-up (as a
/// share of the runner-up's distance) is left unknown. Default portraits
/// have measured 0.83 and under; alternative looks 0.9 and over.
const MAX_CLOSENESS: f32 = 0.85;

/// Internal hero names, for example `npc_dota_hero_phantom_assassin`, empty
/// for a hero that couldn't be told.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lineup {
    pub radiant: [String; 5],
    pub dire: [String; 5],
}

/// The heroes to match against.
#[derive(Debug, Clone)]
pub struct References {
    portraits: Gallery,
    /// The middle of each portrait, to compare with the face in the item
    /// panel.
    faces: Gallery,
}

impl References {
    /// Loads every `<short name>.png` in `dir` (as `fetch-heroes` saves
    /// them), for example `phantom_assassin.png`.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let portraits = Gallery::load(dir, "npc_dota_hero_", THUMB)?;
        let mut heroes = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "png")
                && let Some(short) = path.file_stem().and_then(|s| s.to_str())
            {
                heroes.push((
                    format!("npc_dota_hero_{short}"),
                    image::open(&path)?.into_rgb8(),
                ));
            }
        }
        let mut refs = Self::with_faces(portraits, heroes);
        // Portraits learned from earlier matches: alternative looks that
        // Valve's pictures don't show.
        if let Ok(entries) = std::fs::read_dir(dir.join(LEARNED)) {
            for path in entries.flatten().map(|e| e.path()) {
                if path.extension().is_some_and(|e| e == "png")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    let short = stem.rsplit_once('-').map_or(stem, |(short, _)| short);
                    refs.add(
                        &format!("npc_dota_hero_{short}"),
                        &image::open(&path)?.into_rgb8(),
                    );
                }
            }
        }
        Ok(refs)
    }

    /// Adds another portrait of `hero`, for example an alternative look.
    pub fn add(&mut self, hero: &str, portrait: &RgbImage) {
        self.portraits.push(hero.to_owned(), portrait);
    }

    /// Saves a portrait of `hero` among the learned ones in `dir` (the hero
    /// portraits folder), for [`load`](Self::load) next time, and adds it
    /// now. Returns the file written.
    pub fn learn(
        &mut self,
        dir: &Path,
        hero: &str,
        portrait: &RgbImage,
    ) -> anyhow::Result<PathBuf> {
        let short = hero.strip_prefix("npc_dota_hero_").unwrap_or(hero);
        let learned = dir.join(LEARNED);
        std::fs::create_dir_all(&learned)?;
        let path = (1..)
            .map(|n| learned.join(format!("{short}-{n}.png")))
            .find(|p| !p.exists())
            .expect("a free file name");
        portrait.save(&path)?;
        self.add(hero, portrait);
        Ok(path)
    }

    /// From (internal name, portrait) pairs.
    pub fn from_images(heroes: Vec<(String, RgbImage)>) -> Self {
        Self::with_faces(Gallery::from_images(heroes.clone(), THUMB), heroes)
    }

    fn with_faces(portraits: Gallery, heroes: Vec<(String, RgbImage)>) -> Self {
        let faces = heroes
            .into_iter()
            .map(|(name, image)| {
                let (w, h) = image.dimensions();
                let middle = image::imageops::crop_imm(&image, w / 4, 0, w / 2, h).to_image();
                (name, middle)
            })
            .collect();
        Self {
            portraits,
            faces: Gallery::from_images(faces, FACE),
        }
    }

    pub fn len(&self) -> usize {
        self.portraits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.portraits.len() == 0
    }

    /// Every hero's internal name.
    pub fn names(&self) -> &[String] {
        self.portraits.names()
    }

    /// Which of `candidates` is the hero whose face is in an item-panel
    /// crop at 1080p detail (the selected hero).
    pub fn panel_hero<'a>(&self, panel: &RgbImage, candidates: &'a [String]) -> Option<&'a str> {
        let face = image::imageops::crop_imm(panel, FACE_X, FACE_Y, FACE_W, FACE_H).to_image();
        let (best, _) = self.faces.best_among(&face, candidates)?;
        let name = self.faces.name(best);
        candidates.iter().find(|c| *c == name).map(String::as_str)
    }
    /// The closest hero to each slot's portrait, how far off it is, and how
    /// far off the runner-up is, for tuning.
    #[doc(hidden)]
    pub fn scores(&self, image: &RgbImage) -> [(String, f32, f32); 10] {
        let image = topbar::at_detail(image);
        std::array::from_fn(|i| {
            let p = portrait(&image, i);
            let (best, d) = self.portraits.best(&p).unwrap_or((0, f32::MAX));
            let second = self
                .portraits
                .names()
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != best)
                .filter_map(|(_, n)| self.portraits.distance_to(&p, n))
                .fold(f32::MAX, f32::min);
            (self.portraits.name(best).to_owned(), d, second)
        })
    }

    /// The hero in each slot of one top-bar crop, `None` for dead heroes
    /// (their portraits are grey).
    pub fn read_frame(&self, image: &RgbImage, bar: &TopBar) -> [Option<&str>; 10] {
        let image = topbar::at_detail(image);
        let dead: Vec<bool> = bar
            .radiant
            .iter()
            .chain(&bar.dire)
            .map(|s| s.dead)
            .collect();
        std::array::from_fn(|i| {
            if dead[i] {
                return None;
            }
            let (best, _) = self.portraits.best(&portrait(&image, i))?;
            Some(self.portraits.name(best))
        })
    }
}

/// The portrait in slot `i` (0 to 4 Radiant, 5 to 9 Dire) of a top-bar crop.
pub fn slot_portrait(image: &RgbImage, i: usize) -> RgbImage {
    portrait(&topbar::at_detail(image), i)
}

/// Slot `i` (0 to 4 Radiant, 5 to 9 Dire) of a top-bar crop at 1080p detail.
fn portrait(image: &RgbImage, i: usize) -> RgbImage {
    let x = topbar::slot_x(i < 5, i % 5);
    image::imageops::crop_imm(image, x, PORTRAIT_TOP, PORTRAIT_WIDTH, PORTRAIT_HEIGHT).to_image()
}

/// Reads the lineup by voting over frames, for one match.
#[derive(Debug, Clone)]
pub struct LineupReader {
    refs: References,
    /// For each slot, votes per hero (by index into `refs`), and the sum of
    /// how far off each vote's match was.
    votes: Vec<Vec<u32>>,
    distances: Vec<Vec<f32>>,
    /// The same for the runner-up of each vote.
    runner_up: Vec<Vec<f32>>,
}

impl LineupReader {
    pub fn new(refs: References) -> Self {
        let votes = vec![vec![0; refs.len()]; 10];
        let distances = vec![vec![0.0; refs.len()]; 10];
        let runner_up = distances.clone();
        Self {
            refs,
            votes,
            distances,
            runner_up,
        }
    }

    /// Adds one frame's votes.
    pub fn observe(&mut self, image: &RgbImage, bar: &TopBar) {
        let image = topbar::at_detail(image);
        for (i, slot) in bar.radiant.iter().chain(&bar.dire).enumerate() {
            if slot.dead {
                continue;
            }
            if let Some((best, d, second)) = self.refs.portraits.best_two(&portrait(&image, i)) {
                self.votes[i][best] += 1;
                self.distances[i][best] += d;
                self.runner_up[i][best] += second;
            }
        }
    }

    /// Each slot's leading hero and its share of the votes.
    pub fn leaders(&self) -> [(Option<&str>, f32, u32); 10] {
        std::array::from_fn(|i| {
            let total: u32 = self.votes[i].iter().sum();
            let lead = self.votes[i].iter().enumerate().max_by_key(|&(_, &n)| n);
            match lead {
                Some((hero, &n)) if n > 0 => (
                    Some(self.refs.portraits.name(hero)),
                    n as f32 / total as f32,
                    total,
                ),
                _ => (None, 0.0, total),
            }
        })
    }

    /// The lineup, once every slot has a clear winner. A hero can only play
    /// once, so if two slots read as the same hero, the closer match keeps it
    /// and the other is left empty: a hero missing from the portraits folder
    /// (newer than it) reads as whoever looks most alike.
    pub fn lineup(&self) -> Option<Lineup> {
        let leaders = self.leaders();
        let mut names = Vec::with_capacity(10);
        for (name, share, total) in leaders {
            if total < MIN_VOTES || share < MIN_SHARE {
                return None;
            }
            names.push(name?.to_owned());
        }
        let closeness = |slot: usize| {
            let (hero, &n) = self.votes[slot]
                .iter()
                .enumerate()
                .max_by_key(|&(_, &n)| n)?;
            Some(self.distances[slot][hero] / n.max(1) as f32)
        };
        // A portrait barely closer to its hero than to the next one is an
        // alternative look (an arcana or persona), not that hero.
        for (slot, name) in names.iter_mut().enumerate() {
            if let Some((hero, _)) = self.votes[slot].iter().enumerate().max_by_key(|&(_, &n)| n)
                && self.distances[slot][hero] >= MAX_CLOSENESS * self.runner_up[slot][hero]
            {
                name.clear();
            }
        }
        for a in 0..10 {
            for b in a + 1..10 {
                if !names[a].is_empty() && names[a] == names[b] {
                    let worse = if closeness(a) <= closeness(b) { b } else { a };
                    names[worse].clear();
                }
            }
        }
        let dire = names.split_off(5);
        Some(Lineup {
            radiant: names.try_into().ok()?,
            dire: dire.try_into().ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up portrait: stripes whose direction and colours depend on
    /// `seed`.
    fn art(seed: u32, w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| {
            let (u, v) = (x * 60 / w, y * 36 / h);
            let band = if seed.is_multiple_of(2) {
                (u + seed * 3) / 6
            } else {
                (v + seed) / 4
            };
            let c = (band * 37 + seed * 53) % 200;
            image::Rgb([
                (c + 30) as u8,
                (200 - c) as u8,
                ((c * seed) % 180 + 40) as u8,
            ])
        })
    }

    fn refs() -> References {
        References::from_images(
            (0..12)
                .map(|i| (format!("npc_dota_hero_{i}"), art(i, 256, 144)))
                .collect(),
        )
    }

    /// A top bar with hero `seeds[i]` in slot `i`.
    fn bar(seeds: [u32; 10]) -> RgbImage {
        let mut image =
            RgbImage::from_pixel(topbar::WIDTH, topbar::HEIGHT, image::Rgb([20, 30, 40]));
        for (i, &seed) in seeds.iter().enumerate() {
            let x = topbar::slot_x(i < 5, i % 5);
            image::imageops::replace(
                &mut image,
                &art(seed, PORTRAIT_WIDTH, PORTRAIT_HEIGHT),
                x.into(),
                PORTRAIT_TOP.into(),
            );
        }
        image
    }

    #[test]
    fn reads_each_portrait() {
        let refs = refs();
        let seeds = [3, 1, 4, 10, 5, 9, 2, 6, 0, 7];
        let read = refs.read_frame(&bar(seeds), &TopBar::default());
        for (i, seed) in seeds.iter().enumerate() {
            assert_eq!(read[i], Some(format!("npc_dota_hero_{seed}").as_str()));
        }
    }

    #[test]
    fn learns_an_alternative_look() {
        let dir = std::env::temp_dir().join(format!("dota2coach-learned-{}", std::process::id()));
        let mut refs = refs();
        // A look unlike any reference, learned as hero 3.
        let look = art(40, PORTRAIT_WIDTH, PORTRAIT_HEIGHT);
        let image = bar([40, 1, 4, 10, 5, 9, 2, 6, 0, 7]);
        assert_ne!(
            refs.read_frame(&image, &TopBar::default())[0],
            Some("npc_dota_hero_3")
        );
        refs.learn(&dir, "npc_dota_hero_3", &look).unwrap();
        assert_eq!(
            refs.read_frame(&image, &TopBar::default())[0],
            Some("npc_dota_hero_3")
        );
        assert!(dir.join(LEARNED).join("3-1.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn votes_until_sure() {
        let seeds = [3, 1, 4, 10, 5, 9, 2, 6, 0, 7];
        let image = bar(seeds);
        let mut reader = LineupReader::new(refs());
        let mut dead = TopBar::default();
        dead.dire[2].dead = true;
        for _ in 0..MIN_VOTES - 1 {
            reader.observe(&image, &TopBar::default());
        }
        assert_eq!(reader.lineup(), None, "too few votes");
        // A dead hero's grey portrait doesn't vote.
        reader.observe(&image, &dead);
        assert_eq!(reader.lineup(), None, "slot 8 is one vote short");
        reader.observe(&image, &TopBar::default());
        let lineup = reader.lineup().unwrap();
        assert_eq!(lineup.radiant[0], "npc_dota_hero_3");
        assert_eq!(lineup.dire[4], "npc_dota_hero_7");
    }
}
