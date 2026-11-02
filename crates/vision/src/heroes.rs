//! Reads who is playing from the top bar: matches each hero portrait against
//! Valve's hero portraits, kept in a local folder (`dota2coach fetch-heroes`).
//!
//! One frame can be fooled by a death animation or something drawn over the
//! bar, so [`LineupReader`] votes over many frames and only answers once each
//! slot has a clear winner.

use std::path::Path;

use image::RgbImage;

use crate::topbar::{self, TopBar};

/// The portrait inside a top-bar slot (at 1080p detail): below the player
/// colour strip, and short of the gap to the next portrait.
const PORTRAIT_WIDTH: u32 = 60;
const PORTRAIT_TOP: u32 = 4;
const PORTRAIT_HEIGHT: u32 = 36;
/// Portraits are compared as small thumbnails of this size.
const THUMB_W: u32 = 24;
const THUMB_H: u32 = 14;
/// Rows of the thumbnail compared: the top part, above the marks drawn on
/// your own hero's portrait.
const THUMB_ROWS: u32 = 10;
/// Votes a slot needs, and the share of them its winner needs, before the
/// lineup counts as read.
const MIN_VOTES: u32 = 20;
const MIN_SHARE: f32 = 0.6;

/// Internal hero names, for example `npc_dota_hero_phantom_assassin`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lineup {
    pub radiant: [String; 5],
    pub dire: [String; 5],
}

/// A portrait as a normalised colour thumbnail.
type Thumb = Vec<f32>;

/// The heroes to match against.
#[derive(Debug, Clone, Default)]
pub struct References {
    names: Vec<String>,
    thumbs: Vec<Thumb>,
}

impl References {
    /// Loads every `<short name>.png` in `dir` (as `fetch-heroes` saves
    /// them), for example `phantom_assassin.png`.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let mut heroes = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "png") {
                continue;
            }
            let Some(short) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let image = image::open(&path)?.into_rgb8();
            heroes.push((format!("npc_dota_hero_{short}"), image));
        }
        anyhow::ensure!(!heroes.is_empty(), "no hero portraits in {}", dir.display());
        Ok(Self::from_images(heroes))
    }

    /// From (internal name, portrait) pairs.
    pub fn from_images(heroes: Vec<(String, RgbImage)>) -> Self {
        let mut refs = Self::default();
        for (name, image) in heroes {
            refs.names.push(name);
            refs.thumbs.push(thumb(&image));
        }
        refs
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The closest hero to a portrait, and how far off it is.
    fn best(&self, portrait: &RgbImage) -> Option<(usize, f32)> {
        let t = thumb(portrait);
        self.thumbs
            .iter()
            .map(|r| distance(&t, r))
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(&b.1))
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
            let (best, _) = self.best(&portrait(&image, i))?;
            Some(self.names[best].as_str())
        })
    }
}

/// Slot `i` (0 to 4 Radiant, 5 to 9 Dire) of a top-bar crop at 1080p detail.
fn portrait(image: &RgbImage, i: usize) -> RgbImage {
    let x = topbar::slot_x(i < 5, i % 5);
    image::imageops::crop_imm(image, x, PORTRAIT_TOP, PORTRAIT_WIDTH, PORTRAIT_HEIGHT).to_image()
}

/// A small copy of a portrait, each channel shifted and scaled to mean 0
/// and spread 1 so brightness and contrast differences don't matter.
fn thumb(image: &RgbImage) -> Thumb {
    let small = image::imageops::resize(
        image,
        THUMB_W,
        THUMB_H,
        image::imageops::FilterType::Triangle,
    );
    let mut out = Vec::with_capacity((THUMB_W * THUMB_ROWS * 3) as usize);
    for c in 0..3 {
        let values: Vec<f32> = (0..THUMB_ROWS)
            .flat_map(|y| (0..THUMB_W).map(move |x| (x, y)))
            .map(|(x, y)| f32::from(small.get_pixel(x, y).0[c]))
            .collect();
        let n = values.len() as f32;
        let mean = values.iter().sum::<f32>() / n;
        let spread = (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n)
            .sqrt()
            .max(1.0);
        out.extend(values.iter().map(|v| (v - mean) / spread));
    }
    out
}

fn distance(a: &Thumb, b: &Thumb) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum()
}

/// Reads the lineup by voting over frames, for one match.
#[derive(Debug, Clone)]
pub struct LineupReader {
    refs: References,
    /// For each slot, votes per hero (by index into `refs`).
    votes: Vec<Vec<u32>>,
}

impl LineupReader {
    pub fn new(refs: References) -> Self {
        let votes = vec![vec![0; refs.len()]; 10];
        Self { refs, votes }
    }

    /// Adds one frame's votes.
    pub fn observe(&mut self, image: &RgbImage, bar: &TopBar) {
        let image = topbar::at_detail(image);
        for (i, slot) in bar.radiant.iter().chain(&bar.dire).enumerate() {
            if slot.dead {
                continue;
            }
            if let Some((best, _)) = self.refs.best(&portrait(&image, i)) {
                self.votes[i][best] += 1;
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
                    Some(self.refs.names[hero].as_str()),
                    n as f32 / total as f32,
                    total,
                ),
                _ => (None, 0.0, total),
            }
        })
    }

    /// The lineup, once every slot has a clear winner.
    pub fn lineup(&self) -> Option<Lineup> {
        let leaders = self.leaders();
        let mut names = Vec::with_capacity(10);
        for (name, share, total) in leaders {
            if total < MIN_VOTES || share < MIN_SHARE {
                return None;
            }
            names.push(name?.to_owned());
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
