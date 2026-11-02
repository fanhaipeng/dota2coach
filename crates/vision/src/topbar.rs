//! Reads the top bar: which heroes are dead, and their respawn timers.
//!
//! Works on the top-bar crop [`crate::save_frame`] saves, at 1080p detail
//! (885 by 70 pixels). A dead hero's portrait turns grey and a box with the
//! seconds until respawn hangs below it.

use image::RgbImage;

/// Size of the top-bar crop at 1080p detail.
pub const WIDTH: u32 = 885;
pub const HEIGHT: u32 = 70;

/// Width of one hero portrait.
const SLOT_WIDTH: u32 = 62;
/// Left edge of the first Radiant and first Dire portrait.
const RADIANT_X: u32 = 28;
const DIRE_X: u32 = 548;
/// Upper rows of the portrait, below the player colour strip and above the
/// coloured marks on your own hero's portrait.
const PORTRAIT_ROWS: std::ops::Range<u32> = 5..21;
/// Rows of the digits in the respawn timer box below a dead hero's portrait,
/// short of the box's bottom edge so the world below it stays out.
const TIMER_ROWS: std::ops::Range<u32> = 43..64;
/// A portrait with less colour than this is grey: the hero is dead. Alive
/// portraits measure 15 and up, dead ones close to 0.
const DEAD_COLOUR: u32 = 8;

/// One hero on the top bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Slot {
    pub dead: bool,
    /// Seconds until respawn, if the timer could be read.
    pub respawn: Option<u32>,
}

/// The ten heroes, left to right: Radiant, then Dire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TopBar {
    pub radiant: [Slot; 5],
    pub dire: [Slot; 5],
}

/// Reads a top-bar crop. Crops of another size are scaled to 1080p detail
/// first.
pub fn read(image: &RgbImage) -> TopBar {
    let scaled;
    let image = if image.dimensions() == (WIDTH, HEIGHT) {
        image
    } else {
        scaled =
            image::imageops::resize(image, WIDTH, HEIGHT, image::imageops::FilterType::Triangle);
        &scaled
    };
    let slot = |x: u32| {
        let dead = colour(image, x) < DEAD_COLOUR;
        Slot {
            dead,
            respawn: if dead { read_timer(image, x) } else { None },
        }
    };
    TopBar {
        radiant: std::array::from_fn(|i| slot(slot_x(true, i))),
        dire: std::array::from_fn(|i| slot(slot_x(false, i))),
    }
}

/// Left edge of a hero's portrait, counting from 0 on each side.
#[doc(hidden)]
pub fn slot_x(radiant: bool, index: usize) -> u32 {
    let first = if radiant { RADIANT_X } else { DIRE_X };
    first + index as u32 * SLOT_WIDTH
}

/// Mean colourfulness (max minus min channel) of the portrait starting at `x`.
fn colour(image: &RgbImage, x: u32) -> u32 {
    let (mut sum, mut n) = (0, 0);
    for y in PORTRAIT_ROWS {
        for x in x + 6..x + SLOT_WIDTH - 6 {
            let [r, g, b] = image.get_pixel(x, y).0;
            sum += (r.max(g).max(b) - r.min(g).min(b)) as u32;
            n += 1;
        }
    }
    sum / n
}

/// A digit's shape, scaled to a fixed grid: how much of each cell is ink,
/// from 0 to 9.
#[doc(hidden)]
pub type Glyph = [[u8; GLYPH_W]; GLYPH_H];
const GLYPH_W: usize = 6;
const GLYPH_H: usize = 9;

/// Is this pixel part of the timer's white digits?
fn ink(image: &RgbImage, x: u32, y: u32) -> bool {
    let [r, g, b] = image.get_pixel(x, y).0;
    r.min(g).min(b) > 150
}

/// The digits of the timer below the portrait starting at `x`, as glyphs,
/// left to right.
#[doc(hidden)]
pub fn timer_glyphs(image: &RgbImage, x: u32) -> Vec<Glyph> {
    let columns = x..x + SLOT_WIDTH;
    let inked = |cx: u32| TIMER_ROWS.clone().any(|y| ink(image, cx, y));
    let mut glyphs = Vec::new();
    let mut cx = columns.start;
    while cx < columns.end {
        if !inked(cx) {
            cx += 1;
            continue;
        }
        let left = cx;
        while cx < columns.end && inked(cx) {
            cx += 1;
        }
        let right = cx;
        let rows: Vec<u32> = TIMER_ROWS
            .clone()
            .filter(|&y| (left..right).any(|gx| ink(image, gx, y)))
            .collect();
        let (top, bottom) = (rows[0], rows[rows.len() - 1] + 1);
        // Digits are about 12 pixels tall; skip specks and stray light.
        if !(8..=16).contains(&(bottom - top)) || right - left > 12 {
            continue;
        }
        glyphs.push(normalise(image, left..right, top..bottom));
    }
    glyphs
}

/// How white a pixel is, from 0 to 1, so anti-aliased edges count in part.
fn whiteness(image: &RgbImage, x: f32, y: f32) -> f32 {
    let (x, y) = (x as u32, y as u32);
    if x >= image.width() || y >= image.height() {
        return 0.0;
    }
    let [r, g, b] = image.get_pixel(x, y).0;
    ((r.min(g).min(b) as f32 - 100.0) / 80.0).clamp(0.0, 1.0)
}

/// Averages the ink in a box over the glyph grid. The box is as wide as a
/// digit of that height, centred on the ink, so a narrow 1 stays narrow.
fn normalise(image: &RgbImage, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> Glyph {
    const SAMPLES: usize = 4;
    let h = (ys.end - ys.start) as f32;
    let w = (h * GLYPH_W as f32 / GLYPH_H as f32).max((xs.end - xs.start) as f32);
    let left = (xs.start + xs.end) as f32 / 2.0 - w / 2.0;
    let (cw, ch) = (w / GLYPH_W as f32, h / GLYPH_H as f32);
    let mut glyph = [[0; GLYPH_W]; GLYPH_H];
    for (gy, row) in glyph.iter_mut().enumerate() {
        for (gx, cell) in row.iter_mut().enumerate() {
            let mut sum = 0.0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = left + cw * (gx as f32 + (sx as f32 + 0.5) / SAMPLES as f32);
                    let y = ys.start as f32 + ch * (gy as f32 + (sy as f32 + 0.5) / SAMPLES as f32);
                    if x >= xs.start as f32 {
                        sum += whiteness(image, x, y);
                    }
                }
            }
            *cell = (sum * 9.0 / (SAMPLES * SAMPLES) as f32).round() as u8;
        }
    }
    glyph
}

fn read_timer(image: &RgbImage, x: u32) -> Option<u32> {
    let glyphs = timer_glyphs(image, x);
    if glyphs.is_empty() || glyphs.len() > 3 {
        return None;
    }
    glyphs.iter().try_fold(0, |n, g| Some(n * 10 + digit(g)?))
}

/// The digit a glyph shows, if it is close enough to one.
fn digit(glyph: &Glyph) -> Option<u32> {
    let mut ranked: Vec<(u32, u32)> = DIGITS
        .iter()
        .enumerate()
        .map(|(d, t)| (distance(glyph, t), d as u32))
        .collect();
    ranked.sort();
    let [(best, d), (second, _), ..] = ranked[..] else {
        return None;
    };
    (best <= MAX_DISTANCE && second - best >= MIN_LEAD).then_some(d)
}

/// How much closer the best digit must be than the runner-up.
const MIN_LEAD: u32 = 10;

/// In ninths of a cell, out of 54 cells: about 13 cells wholly wrong. Lone
/// digits drift further from the templates than digits in pairs.
const MAX_DISTANCE: u32 = 100;

fn distance(glyph: &Glyph, template: &[&str; GLYPH_H]) -> u32 {
    let mut n = 0;
    for (row, t) in glyph.iter().zip(template) {
        for (&cell, tc) in row.iter().zip(t.bytes()) {
            n += (cell as u32).abs_diff((tc - b'0') as u32);
        }
    }
    n
}

/// 0 to 9, learned from our own hero's timer in a recorded match (the
/// topbar example's `--glyphs`): the mean ink of each cell, from 0 to 9.
#[rustfmt::skip]
const DIGITS: [[&str; GLYPH_H]; 10] = [
    // 0: 13 samples
    [
        "289982",
        "795696",
        "971288",
        "970079",
        "970079",
        "970079",
        "882388",
        "597894",
        "169951",
    ],
    // 1: 69 samples
    [
        "019970",
        "078970",
        "055970",
        "001970",
        "001970",
        "001970",
        "001970",
        "002970",
        "001970",
    ],
    // 2: 45 samples
    [
        "999982",
        "777894",
        "000595",
        "001594",
        "003892",
        "017961",
        "279610",
        "799877",
        "999999",
    ],
    // 3: 38 samples
    [
        "399982",
        "244893",
        "001593",
        "026961",
        "039961",
        "015793",
        "000595",
        "277894",
        "399940",
    ],
    // 4: 30 samples
    [
        "000582",
        "004882",
        "017982",
        "156882",
        "574783",
        "987996",
        "546996",
        "113783",
        "001782",
    ],
    // 5: 31 samples
    [
        "299994",
        "296331",
        "295100",
        "498651",
        "257995",
        "012698",
        "000599",
        "777894",
        "899940",
    ],
    // 6: 26 samples
    [
        "017920",
        "158400",
        "497410",
        "899963",
        "996697",
        "982289",
        "970079",
        "887788",
        "289972",
    ],
    // 7: 25 samples
    [
        "999999",
        "666894",
        "001683",
        "004951",
        "016930",
        "028710",
        "159500",
        "287300",
        "595100",
    ],
    // 8: 20 samples
    [
        "289993",
        "785497",
        "783288",
        "387793",
        "388894",
        "784388",
        "881079",
        "787799",
        "179972",
    ],
    // 9: 25 samples
    [
        "047740",
        "497893",
        "783387",
        "771177",
        "698896",
        "067994",
        "003880",
        "027940",
        "069510",
    ],
];

/// Renders a glyph as text, for building [`DIGITS`].
#[doc(hidden)]
pub fn show(glyph: &Glyph) -> [String; GLYPH_H] {
    std::array::from_fn(|y| glyph[y].iter().map(|&c| char::from(b'0' + c)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREY: [u8; 3] = [70, 70, 70];
    const DARK: [u8; 3] = [30, 32, 36];

    /// A top bar of colourful portraits.
    fn bar() -> RgbImage {
        let mut image = RgbImage::from_pixel(WIDTH, HEIGHT, image::Rgb([40, 60, 50]));
        for i in 0..10 {
            let x = if i < 5 { RADIANT_X } else { DIRE_X } + (i % 5) * SLOT_WIDTH;
            fill(
                &mut image,
                x,
                0,
                SLOT_WIDTH - 1,
                40,
                [180, 90 + i as u8 * 10, 40],
            );
        }
        image
    }

    fn fill(image: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, rgb: [u8; 3]) {
        for y in y..y + h {
            for x in x..x + w {
                image.put_pixel(x, y, image::Rgb(rgb));
            }
        }
    }

    /// Greys out a portrait and draws `seconds` in the box below it, a pixel
    /// for each cell of the digit templates.
    fn kill(image: &mut RgbImage, x: u32, seconds: &str) {
        fill(image, x, 0, SLOT_WIDTH - 1, 40, GREY);
        fill(image, x + 6, 42, 42, 24, DARK);
        let mut left = x + 27 - seconds.len() as u32 * 4;
        for d in seconds.bytes() {
            let template = &DIGITS[(d - b'0') as usize];
            for (gy, row) in template.iter().enumerate() {
                for (gx, level) in row.bytes().enumerate() {
                    // As white as the cell was inked on average.
                    let v = (100 + (level - b'0') as u32 * 80 / 9) as u8;
                    if level > b'0' {
                        image.put_pixel(left + gx as u32, 48 + gy as u32, image::Rgb([v, v, v]));
                    }
                }
            }
            left += 8;
        }
    }

    #[test]
    fn reads_every_digit_from_its_own_template() {
        for (d, template) in DIGITS.iter().enumerate() {
            let glyph: Glyph =
                std::array::from_fn(|y| std::array::from_fn(|x| template[y].as_bytes()[x] - b'0'));
            assert_eq!(digit(&glyph), Some(d as u32));
        }
    }

    #[test]
    fn everyone_alive() {
        assert_eq!(read(&bar()), TopBar::default());
    }

    #[test]
    fn reads_dead_heroes_and_their_timers() {
        let mut image = bar();
        kill(&mut image, RADIANT_X, "7");
        kill(&mut image, RADIANT_X + 2 * SLOT_WIDTH, "45");
        kill(&mut image, DIRE_X + 4 * SLOT_WIDTH, "18");
        let read = read(&image);
        assert_eq!(
            read.radiant[0],
            Slot {
                dead: true,
                respawn: Some(7)
            }
        );
        assert!(!read.radiant[1].dead);
        assert_eq!(
            read.radiant[2],
            Slot {
                dead: true,
                respawn: Some(45)
            }
        );
        assert_eq!(
            read.dire[4],
            Slot {
                dead: true,
                respawn: Some(18)
            }
        );
        assert_eq!(read.dire[..4], [Slot::default(); 4]);
    }

    #[test]
    fn a_dead_hero_without_a_readable_timer() {
        let mut image = bar();
        fill(&mut image, DIRE_X, 0, SLOT_WIDTH - 1, 40, GREY);
        assert_eq!(
            read(&image).dire[0],
            Slot {
                dead: true,
                respawn: None
            }
        );
    }

    #[test]
    fn scales_other_crop_sizes() {
        let mut image = bar();
        fill(&mut image, DIRE_X + SLOT_WIDTH, 0, SLOT_WIDTH - 1, 40, GREY);
        // A 1440p screen's crop.
        let big = image::imageops::resize(&image, 1180, 93, image::imageops::FilterType::Nearest);
        let read = read(&big);
        assert!(read.dire[1].dead);
        assert_eq!(read.dire.iter().filter(|s| s.dead).count(), 1);
        assert!(read.radiant.iter().all(|s| !s.dead));
    }
}
