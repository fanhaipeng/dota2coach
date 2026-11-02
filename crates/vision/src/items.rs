//! Reads the items in the ability and item panel (the `-items` crop, 1080 by
//! 194 at 1080p detail): yours, or those of the hero you have selected.
//! Icons are matched against Valve's item icons, kept in a local folder
//! (`dota2coach fetch-items`).

use std::path::Path;

use image::RgbImage;

use crate::gallery::{Gallery, ThumbSize};

/// Size of the panel crop at 1080p detail.
pub const WIDTH: u32 = 1080;
pub const HEIGHT: u32 = 194;

/// Where the six inventory slots are, two rows of three, at 1080p detail.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub x: u32,
    pub y: u32,
    /// From one slot to the next, across and down.
    pub dx: u32,
    pub dy: u32,
    /// The icon inside a slot.
    pub w: u32,
    pub h: u32,
}

/// Measured on 1080p-detail crops against the items GSI reported, with the
/// ability row of a hero with four abilities; [`Icons::find_grid`] finds the
/// shift for others.
pub const INVENTORY: Grid = Grid {
    x: 737,
    y: 58,
    dx: 66,
    dy: 49,
    w: 58,
    h: 42,
};

const THUMB: ThumbSize = ThumbSize {
    w: 16,
    h: 12,
    rows: 12,
};

/// How far the inventory moves sideways, either way, in pixels.
const MAX_SHIFT: i32 = 64;
/// An icon matching its item worse than this is something else (a cooldown
/// darkening it, an item missing from the folder).
const MAX_DISTANCE: f32 = 500.0;
/// An empty slot is a flat dark square; icons are busy.
const MIN_BUSYNESS: f32 = 12.0;

impl Grid {
    fn shifted(&self, dx: i32) -> Grid {
        Grid {
            x: (self.x as i32 + dx).max(0) as u32,
            ..*self
        }
    }

    /// Inventory slot `i` (0 to 5, as GSI numbers them: left to right, top
    /// row first).
    #[doc(hidden)]
    pub fn slot(&self, image: &RgbImage, i: u32) -> RgbImage {
        let (col, row) = (i % 3, i / 3);
        image::imageops::crop_imm(
            image,
            self.x + col * self.dx,
            self.y + row * self.dy,
            self.w,
            self.h,
        )
        .to_image()
    }
}

/// Valve's item icons to match against.
#[derive(Debug, Clone)]
pub struct Icons(Gallery);

impl Icons {
    /// Loads every `<short name>.png` in `dir` (as `fetch-items` saves
    /// them), naming each `item_<short name>` as GSI does.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        Ok(Self(Gallery::load(dir, "item_", THUMB)?))
    }

    pub fn from_images(icons: Vec<(String, RgbImage)>) -> Self {
        Self(Gallery::from_images(icons, THUMB))
    }

    /// The closest item to an icon, and how far off it is.
    #[doc(hidden)]
    pub fn best(&self, icon: &RgbImage) -> Option<(&str, f32)> {
        let (i, d) = self.0.best(icon)?;
        Some((self.0.name(i), d))
    }

    /// How far an icon is from the named item's.
    #[doc(hidden)]
    pub fn distance_to(&self, icon: &RgbImage, item: &str) -> Option<f32> {
        self.0.distance_to(icon, item)
    }

    /// The six inventory items in a panel crop, `None` for empty slots and
    /// icons that match nothing well.
    pub fn inventory(&self, panel: &RgbImage) -> [Option<&str>; 6] {
        let panel = at_detail(panel);
        let grid = self.find_grid(&panel);
        std::array::from_fn(|i| {
            let icon = grid.slot(&panel, i as u32);
            if busyness(&icon) < MIN_BUSYNESS {
                return None;
            }
            let (item, distance) = self.best(&icon)?;
            (distance <= MAX_DISTANCE).then_some(item)
        })
    }

    /// The inventory grid, which moves sideways as the hero's ability row
    /// grows or shrinks: the shift whose slots best match some icon.
    #[doc(hidden)]
    pub fn find_grid(&self, panel: &RgbImage) -> Grid {
        let fit = |shift: i32| -> f32 {
            let grid = INVENTORY.shifted(shift);
            (0..6)
                .map(|i| self.best(&grid.slot(panel, i)).map_or(f32::MAX, |b| b.1))
                .sum()
        };
        let coarse = (-MAX_SHIFT..=MAX_SHIFT)
            .step_by(4)
            .min_by(|a, b| fit(*a).total_cmp(&fit(*b)))
            .unwrap_or(0);
        let fine = (coarse - 3..=coarse + 3)
            .min_by(|a, b| fit(*a).total_cmp(&fit(*b)))
            .unwrap_or(coarse);
        INVENTORY.shifted(fine)
    }
}

/// Whose hero the panel shows, from its health bar: green for you and your
/// team, red for enemies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ally,
    Enemy,
}

/// The rows and columns the health bar can be in (it moves sideways with
/// the ability row), and how many of its pixels must show a clear colour.
const HEALTH_ROWS: std::ops::Range<u32> = 134..156;
const HEALTH_COLUMNS: std::ops::Range<u32> = 250..760;
const MIN_HEALTH_PIXELS: usize = 300;

/// The side of the hero in a panel crop, `None` if no health bar is clear
/// (for example while you are dead).
pub fn side(panel: &RgbImage) -> Option<Side> {
    let panel = at_detail(panel);
    let (mut green, mut red) = (0, 0);
    for y in HEALTH_ROWS {
        for x in HEALTH_COLUMNS {
            let [r, g, b] = panel.get_pixel(x, y).0.map(i32::from);
            if g > 100 && g > r + 40 && g > b + 20 {
                green += 1;
            } else if r > 110 && r > g + 60 && r > b + 40 {
                red += 1;
            }
        }
    }
    match (green, red) {
        (g, r) if g >= MIN_HEALTH_PIXELS && g > r => Some(Side::Ally),
        (g, r) if r >= MIN_HEALTH_PIXELS && r > g => Some(Side::Enemy),
        _ => None,
    }
}

/// Where the selected hero's name is written, above its face (at 1080p
/// detail), and how much it is enlarged for text recognition.
const NAME_X: u32 = 120;
const NAME_Y: u32 = 26;
const NAME_W: u32 = 160;
const NAME_H: u32 = 24;
const NAME_ZOOM: u32 = 4;
/// White space around the letters, which text recognition needs.
const NAME_BORDER: u32 = 40;

/// The selected hero's name from a panel crop, enlarged for
/// [`crate::ocr::read_text`]: as it looks, and with only its letters kept
/// (your own name is gold, others' white), black on a white border, at two
/// sizes. The world shows through behind the name, and short names need room
/// around them, so any of them can read best.
pub fn name_pictures(panel: &RgbImage) -> [RgbImage; 3] {
    let panel = at_detail(panel);
    let strip = image::imageops::crop_imm(&*panel, NAME_X, NAME_Y, NAME_W, NAME_H).to_image();
    let zoom = |z: u32| {
        image::imageops::resize(
            &strip,
            NAME_W * z,
            NAME_H * z,
            image::imageops::FilterType::CatmullRom,
        )
    };
    let letters = |z: u32| {
        let big = zoom(z);
        let mut out = RgbImage::from_pixel(
            big.width() + 2 * NAME_BORDER,
            big.height() + 2 * NAME_BORDER,
            image::Rgb([255, 255, 255]),
        );
        for (x, y, p) in big.enumerate_pixels() {
            let [r, g, b] = p.0.map(i32::from);
            let gold = r > 150 && g > 110 && b < 150 && r - b > 50;
            let white = r.min(g).min(b) > 170 && r.max(g).max(b) - r.min(g).min(b) < 45;
            if gold || white {
                out.put_pixel(x + NAME_BORDER, y + NAME_BORDER, image::Rgb([0, 0, 0]));
            }
        }
        out
    };
    [zoom(NAME_ZOOM), letters(3), letters(6)]
}
/// The spread of brightness across a picture.
fn busyness(image: &RgbImage) -> f32 {
    let values: Vec<f32> = image
        .pixels()
        .map(|p| {
            let [r, g, b] = p.0.map(f32::from);
            0.3 * r + 0.59 * g + 0.11 * b
        })
        .collect();
    let n = values.len().max(1) as f32;
    let mean = values.iter().sum::<f32>() / n;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt()
}

/// A panel crop at 1080p detail, scaled if it isn't already.
pub fn at_detail(image: &RgbImage) -> std::borrow::Cow<'_, RgbImage> {
    if image.dimensions() == (WIDTH, HEIGHT) {
        std::borrow::Cow::Borrowed(image)
    } else {
        std::borrow::Cow::Owned(image::imageops::resize(
            image,
            WIDTH,
            HEIGHT,
            image::imageops::FilterType::Triangle,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up icon: stripes whose direction and colours depend on `seed`.
    fn icon(seed: u32, w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| {
            let (u, v) = (x * 88 / w, y * 64 / h);
            let band = if seed.is_multiple_of(2) {
                (u + seed * 5) / 9
            } else {
                (v + seed * 3) / 7
            };
            let c = (band * 41 + seed * 59) % 200;
            image::Rgb([
                (c + 30) as u8,
                (210 - c) as u8,
                ((c * seed) % 170 + 50) as u8,
            ])
        })
    }

    fn icons() -> Icons {
        Icons::from_images(
            (0..10)
                .map(|i| (format!("item_{i}"), icon(i, 88, 64)))
                .collect(),
        )
    }

    /// A panel with item `seeds[i]` in slot `i` (`None` empty), the grid
    /// shifted by `shift`, and a health bar of `bar`.
    fn panel(seeds: [Option<u32>; 6], shift: i32, bar: [u8; 3]) -> RgbImage {
        let mut image = RgbImage::from_pixel(WIDTH, HEIGHT, image::Rgb([25, 28, 30]));
        let grid = INVENTORY.shifted(shift);
        for (i, seed) in seeds.iter().enumerate() {
            let Some(seed) = seed else { continue };
            let (col, row) = (i as u32 % 3, i as u32 / 3);
            let art = icon(*seed, grid.w, grid.h);
            image::imageops::replace(
                &mut image,
                &art,
                (grid.x + col * grid.dx).into(),
                (grid.y + row * grid.dy).into(),
            );
        }
        for y in HEALTH_ROWS {
            for x in 300..600 {
                image.put_pixel(x, y, image::Rgb(bar));
            }
        }
        image
    }

    #[test]
    fn reads_the_inventory_where_it_has_moved() {
        let seeds = [Some(3), Some(7), None, Some(0), Some(9), Some(4)];
        let icons = icons();
        for shift in [0, -16, 28] {
            let read = icons.inventory(&panel(seeds, shift, [40, 200, 40]));
            let want: Vec<Option<String>> = seeds
                .iter()
                .map(|s| s.map(|s| format!("item_{s}")))
                .collect();
            let got: Vec<Option<String>> = read.iter().map(|r| r.map(str::to_owned)).collect();
            assert_eq!(got, want, "shift {shift}");
        }
    }

    #[test]
    fn tells_allies_from_enemies() {
        let empty = [None; 6];
        assert_eq!(side(&panel(empty, 0, [40, 200, 40])), Some(Side::Ally));
        assert_eq!(side(&panel(empty, 0, [210, 40, 30])), Some(Side::Enemy));
        assert_eq!(side(&panel(empty, 0, [25, 28, 30])), None);
    }
}
