//! Reads the minimap: where the heroes you can see are, allies and enemies.
//!
//! Works on the minimap crop [`crate::save_frame`] saves, at 1080p detail
//! (259 by 259 pixels). Each hero icon has an arrow around it in its team's
//! colour, green for allies and red for enemies, pointing where the hero
//! faces. Creeps are small squares of the same colours and buildings never
//! move, so the reader keeps the blobs that are arrow-shaped and not
//! standing in one place for long.

use image::RgbImage;

/// Size of the minimap crop at 1080p detail.
pub const SIZE: u32 = 259;

/// Pixels per world unit, and where world (0, 0) lands, measured from your
/// own hero's GSI position against its icon.
const SCALE: f32 = 0.01372;
const ORIGIN_X: f32 = 121.6;
const ORIGIN_Y: f32 = 136.8;

/// A team-coloured pixel that stays put for this many reads is part of a
/// building (or a hero that hasn't moved in a long time).
const STATIC_READS: u16 = 40;
const STILL_CAP: u16 = 200;
const STILL_DROP: u16 = 4;
/// How far an arrow sits from the middle of its hero's icon, in pixels.
const ICON_OFFSET: f32 = 12.0;
/// Blobs of team colour smaller than this are creeps or specks.
const MIN_PIXELS: usize = 12;
/// Blobs filling more of their box than this are creep squares or towers;
/// arrows are triangles.
const MAX_FILL: f32 = 0.55;
/// A hero portrait's radius, and how busy (spread of brightness) the picture
/// inside it must be: faces are busy, the map behind a creep is not.
const PORTRAIT_RADIUS: f32 = 5.0;
const MIN_BUSYNESS: f32 = 24.0;
/// Heroes found closer than this are one hero: an arrow can break into two
/// pieces, each fitted to its own ring. Two heroes standing on top of each
/// other also count once.
const SAME_HERO: f32 = 12.0;

/// A point on the map in world units: about -8000 to 8000 on each axis,
/// Radiant bottom left, the same as GSI's `xpos` and `ypos`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i64,
    pub y: i64,
}

impl Point {
    pub fn distance(self, other: Point) -> f64 {
        (((self.x - other.x).pow(2) + (self.y - other.y).pow(2)) as f64).sqrt()
    }
}

/// The heroes seen on one minimap.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Minimap {
    /// Your team, you included.
    pub allies: Vec<Point>,
    pub enemies: Vec<Point>,
}

/// Where a minimap pixel is in world units.
pub fn to_world(px: f32, py: f32) -> Point {
    Point {
        x: ((px - ORIGIN_X) / SCALE).round() as i64,
        y: ((ORIGIN_Y - py) / SCALE).round() as i64,
    }
}

/// Where a world point is on the minimap, in pixels.
pub fn to_pixel(p: Point) -> (f32, f32) {
    (ORIGIN_X + p.x as f32 * SCALE, ORIGIN_Y - p.y as f32 * SCALE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Team {
    Ally,
    Enemy,
}

fn team(rgb: [u8; 3]) -> Option<Team> {
    let [r, g, b] = rgb.map(i32::from);
    if r > 170 && g < 80 && b < 80 {
        Some(Team::Enemy)
    } else if g > 150 && r < 120 && b < 110 && g - r > 70 {
        Some(Team::Ally)
    } else {
        None
    }
}

/// Reads minimaps one after another, learning which coloured pixels are
/// buildings because they never move. Feed it every reading of one match.
#[derive(Debug, Clone)]
pub struct Reader {
    /// For each pixel, how many reads in a row it has been team-coloured.
    still: Vec<u16>,
}

impl Default for Reader {
    fn default() -> Self {
        Self {
            still: vec![0; (SIZE * SIZE) as usize],
        }
    }
}

/// A blob of one team's colour.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct Blob {
    pub ally: bool,
    pub x: f32,
    pub y: f32,
    /// The middle of the blob's own pixels.
    pub middle: (f32, f32),
    pub pixels: usize,
    pub fill: f32,
}

impl Reader {
    /// Reads a minimap crop. Crops of another size are scaled to 1080p
    /// detail first.
    pub fn read(&mut self, image: &RgbImage) -> Minimap {
        let mut map = Minimap::default();
        for hero in self.arrows(image) {
            let at = to_world(hero.x, hero.y);
            if hero.ally {
                map.allies.push(at);
            } else {
                map.enemies.push(at);
            }
        }
        map
    }

    /// The hero arrows on a crop, merged so each hero is one blob.
    #[doc(hidden)]
    pub fn arrows(&mut self, image: &RgbImage) -> Vec<Blob> {
        let scaled;
        let image = if image.dimensions() == (SIZE, SIZE) {
            image
        } else {
            scaled =
                image::imageops::resize(image, SIZE, SIZE, image::imageops::FilterType::Triangle);
            &scaled
        };
        let teams: Vec<Option<Team>> = image.pixels().map(|p| team(p.0)).collect();
        // JPEG noise flickers building pixels in and out of colour, so a miss
        // only takes a little off the count.
        for (still, t) in self.still.iter_mut().zip(&teams) {
            *still = if t.is_some() {
                (*still + 1).min(STILL_CAP)
            } else {
                still.saturating_sub(STILL_DROP)
            };
        }
        let moving: Vec<Option<Team>> = teams
            .iter()
            .zip(&self.still)
            .map(|(t, &still)| t.filter(|_| still < STATIC_READS))
            .collect();
        let pieces: Vec<Blob> = blobs(&moving, ICON_OFFSET)
            .into_iter()
            .filter(|b| b.pixels >= MIN_PIXELS && b.fill <= MAX_FILL)
            .filter_map(|b| on_portrait(image, b))
            .collect();
        merge(pieces)
    }
}

/// Connected blobs of one team's colour, each placed at the middle of the
/// circle of `radius` that best fits its pixels: for an arrow, the middle of
/// its hero's icon.
fn blobs(teams: &[Option<Team>], radius: f32) -> Vec<Blob> {
    let n = SIZE as usize;
    let mut seen = vec![false; teams.len()];
    let mut out = Vec::new();
    for start in 0..teams.len() {
        let Some(t) = teams[start] else { continue };
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut pixels = Vec::new();
        let (mut count, mut sx, mut sy) = (0usize, 0usize, 0usize);
        let (mut x0, mut x1, mut y0, mut y1) = (n, 0, n, 0);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % n, i / n);
            pixels.push((x, y));
            count += 1;
            sx += x;
            sy += y;
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
            let mut visit = |j: usize| {
                if !seen[j] && teams[j] == Some(t) {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < n {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - n);
            }
            if y + 1 < n {
                visit(i + n);
            }
        }
        let (cx, cy) = (sx as f32 / count as f32, sy as f32 / count as f32);
        if encloses(&pixels, (x0, y0, x1, y1), (cx, cy)) {
            // A closed ring is an observer or sentry ward, not an arrow.
            continue;
        }
        let (x, y) = ring_centre(&pixels, (cx, cy), radius);
        out.push(Blob {
            ally: t == Team::Ally,
            x,
            y,
            middle: (cx, cy),
            pixels: count,
            fill: count as f32 / ((x1 - x0 + 1) * (y1 - y0 + 1)) as f32,
        });
    }
    out
}

/// Whether a blob surrounds its own middle, which it doesn't cover: looking
/// left, right, up and down from there, the blob is in every direction.
fn encloses(
    pixels: &[(usize, usize)],
    (x0, y0, x1, y1): (usize, usize, usize, usize),
    (cx, cy): (f32, f32),
) -> bool {
    let w = x1 - x0 + 1;
    let mut mask = vec![false; w * (y1 - y0 + 1)];
    for &(x, y) in pixels {
        mask[(y - y0) * w + x - x0] = true;
    }
    let at = |x: usize, y: usize| mask[(y - y0) * w + x - x0];
    let (mx, my) = (
        (cx.round() as usize).clamp(x0, x1),
        (cy.round() as usize).clamp(y0, y1),
    );
    !at(mx, my)
        && (x0..mx).any(|x| at(x, my))
        && (mx + 1..=x1).any(|x| at(x, my))
        && (y0..my).any(|y| at(mx, y))
        && (my + 1..=y1).any(|y| at(mx, y))
}

/// Keeps a blob only if a hero portrait sits in the middle of its ring. The
/// ring fit can land on the wrong side of a straight arrow, so if there's no
/// portrait there the mirror image across the arrow is tried.
fn on_portrait(image: &RgbImage, mut blob: Blob) -> Option<Blob> {
    let mirror = (2.0 * blob.middle.0 - blob.x, 2.0 * blob.middle.1 - blob.y);
    if busyness(image, blob.x, blob.y, PORTRAIT_RADIUS) >= MIN_BUSYNESS {
        return Some(blob);
    }
    (blob.x, blob.y) = mirror;
    (busyness(image, mirror.0, mirror.1, PORTRAIT_RADIUS) >= MIN_BUSYNESS).then_some(blob)
}

/// How busy the picture is within `r` pixels of a point: the spread of
/// brightness. Hero portraits are busy; the map behind a creep is not.
#[doc(hidden)]
pub fn busyness(image: &RgbImage, x: f32, y: f32, r: f32) -> f32 {
    let mut values = Vec::new();
    let ri = r.ceil() as i32;
    for dy in -ri..=ri {
        for dx in -ri..=ri {
            let (px, py) = (x.round() as i32 + dx, y.round() as i32 + dy);
            if (dx * dx + dy * dy) as f32 > r * r
                || px < 0
                || py < 0
                || px >= image.width() as i32
                || py >= image.height() as i32
            {
                continue;
            }
            let [cr, cg, cb] = image.get_pixel(px as u32, py as u32).0.map(f32::from);
            values.push(0.3 * cr + 0.59 * cg + 0.11 * cb);
        }
    }
    let n = values.len().max(1) as f32;
    let mean = values.iter().sum::<f32>() / n;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt()
}

/// The point near `around` from which `pixels` are closest to `radius` away.
/// An arrow hugs its icon's edge, so this is the icon's middle.
fn ring_centre(pixels: &[(usize, usize)], around: (f32, f32), radius: f32) -> (f32, f32) {
    let cost = |cx: f32, cy: f32| -> f32 {
        pixels
            .iter()
            .map(|&(x, y)| ((x as f32 - cx).hypot(y as f32 - cy) - radius).powi(2))
            .sum()
    };
    let mut best = (around, f32::MAX);
    for dy in -24..=24 {
        for dx in -24..=24 {
            let c = (around.0 + dx as f32 * 0.5, around.1 + dy as f32 * 0.5);
            let cost = cost(c.0, c.1);
            if cost < best.1 {
                best = (c, cost);
            }
        }
    }
    best.0
}

/// Joins arrow pieces of the same team that are close together.
fn merge(pieces: Vec<Blob>) -> Vec<Blob> {
    let mut heroes: Vec<Blob> = Vec::new();
    for piece in pieces {
        let near = heroes
            .iter_mut()
            .find(|h| h.ally == piece.ally && (h.x - piece.x).hypot(h.y - piece.y) < SAME_HERO);
        match near {
            Some(h) => {
                let total = (h.pixels + piece.pixels) as f32;
                h.x = (h.x * h.pixels as f32 + piece.x * piece.pixels as f32) / total;
                h.y = (h.y * h.pixels as f32 + piece.y * piece.pixels as f32) / total;
                h.pixels += piece.pixels;
            }
            None => heroes.push(piece),
        }
    }
    heroes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain dark map.
    fn map() -> RgbImage {
        RgbImage::from_pixel(SIZE, SIZE, image::Rgb([40, 50, 45]))
    }

    /// A hero at (x, y): a busy portrait with an arrow of `rgb` around its
    /// right side.
    fn hero(image: &mut RgbImage, x: f32, y: f32, rgb: [u8; 3]) {
        for py in 0..SIZE {
            for px in 0..SIZE {
                let (dx, dy) = (px as f32 - x, py as f32 - y);
                let d = dx.hypot(dy);
                if d <= 6.0 {
                    let v = if (px + py) % 2 == 0 { 230 } else { 20 };
                    image.put_pixel(px, py, image::Rgb([v, v, v]));
                } else if (d - ICON_OFFSET).abs() < 1.5 && dy.atan2(dx).abs() < 0.9 {
                    image.put_pixel(px, py, image::Rgb(rgb));
                }
            }
        }
    }

    fn square(image: &mut RgbImage, x: u32, y: u32, rgb: [u8; 3]) {
        for py in y..y + 6 {
            for px in x..x + 6 {
                image.put_pixel(px, py, image::Rgb(rgb));
            }
        }
    }

    #[test]
    fn converts_between_pixels_and_world_units() {
        let p = Point { x: 5153, y: -5854 };
        let (px, py) = to_pixel(p);
        assert!((190.0..195.0).contains(&px) && (215.0..220.0).contains(&py));
        let back = to_world(px, py);
        assert!(back.distance(p) < 2.0, "{back:?}");
    }

    #[test]
    fn finds_heroes_and_skips_creeps() {
        let mut image = map();
        hero(&mut image, 100.0, 120.0, [40, 230, 40]);
        hero(&mut image, 180.0, 60.0, [230, 30, 30]);
        // Creeps of both teams.
        square(&mut image, 30, 200, [40, 230, 40]);
        square(&mut image, 210, 30, [230, 30, 30]);

        let read = Reader::default().read(&image);
        assert_eq!(read.allies.len(), 1, "{read:?}");
        assert_eq!(read.enemies.len(), 1, "{read:?}");
        assert!(
            read.allies[0].distance(to_world(100.0, 120.0)) < 150.0,
            "{read:?}"
        );
        assert!(
            read.enemies[0].distance(to_world(180.0, 60.0)) < 150.0,
            "{read:?}"
        );
    }

    #[test]
    fn skips_wards() {
        let mut image = map();
        // A closed green ring around a busy middle, like an observer ward.
        for py in 0..SIZE {
            for px in 0..SIZE {
                let d = (px as f32 - 60.0).hypot((py as f32 - 60.0) * 1.6);
                if d < 4.0 {
                    let v = if (px + py) % 2 == 0 { 230 } else { 20 };
                    image.put_pixel(px, py, image::Rgb([v, v, v]));
                } else if (d - 6.0).abs() < 1.2 {
                    image.put_pixel(px, py, image::Rgb([40, 230, 40]));
                }
            }
        }
        assert_eq!(Reader::default().read(&image), Minimap::default());
    }

    #[test]
    fn learns_that_buildings_dont_move() {
        let mut image = map();
        hero(&mut image, 180.0, 60.0, [230, 30, 30]);
        let mut reader = Reader::default();
        assert_eq!(reader.read(&image).enemies.len(), 1);
        for _ in 0..STATIC_READS {
            reader.read(&image);
        }
        assert!(reader.read(&image).enemies.is_empty());
    }

    #[test]
    fn scales_other_crop_sizes() {
        let mut image = map();
        hero(&mut image, 100.0, 120.0, [40, 230, 40]);
        let big = image::imageops::resize(&image, 346, 346, image::imageops::FilterType::Nearest);
        assert_eq!(Reader::default().read(&big).allies.len(), 1);
    }
}
