//! Matching small pictures (hero portraits, item icons) against a folder of
//! Valve's images, as normalised colour thumbnails.

use std::path::Path;

use image::RgbImage;

/// The thumbnail pictures are compared at: `w` by `h`, of which the top
/// `rows` count.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThumbSize {
    pub w: u32,
    pub h: u32,
    pub rows: u32,
}

/// A picture as a normalised colour thumbnail.
type Thumb = Vec<f32>;

/// Named pictures to match against.
#[derive(Debug, Clone)]
pub(crate) struct Gallery {
    size: ThumbSize,
    names: Vec<String>,
    thumbs: Vec<Thumb>,
}

impl Gallery {
    /// Loads every `<short name>.png` in `dir`, naming each `<prefix><short
    /// name>`.
    pub fn load(dir: &Path, prefix: &str, size: ThumbSize) -> anyhow::Result<Self> {
        let mut pictures = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "png") {
                continue;
            }
            let Some(short) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let image = image::open(&path)?.into_rgb8();
            pictures.push((format!("{prefix}{short}"), image));
        }
        anyhow::ensure!(!pictures.is_empty(), "no pictures in {}", dir.display());
        Ok(Self::from_images(pictures, size))
    }

    pub fn from_images(pictures: Vec<(String, RgbImage)>, size: ThumbSize) -> Self {
        let mut gallery = Self {
            size,
            names: Vec::new(),
            thumbs: Vec::new(),
        };
        for (name, image) in pictures {
            gallery.thumbs.push(thumb(&image, size));
            gallery.names.push(name);
        }
        gallery
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn name(&self, i: usize) -> &str {
        &self.names[i]
    }

    /// How far a picture is from the one named `name`.
    pub fn distance_to(&self, picture: &RgbImage, name: &str) -> Option<f32> {
        let i = self.names.iter().position(|n| n == name)?;
        Some(distance(&thumb(picture, self.size), &self.thumbs[i]))
    }

    /// The closest of the pictures named in `among`, and how far off it is.
    pub fn best_among(&self, picture: &RgbImage, among: &[String]) -> Option<(usize, f32)> {
        let t = thumb(picture, self.size);
        self.names
            .iter()
            .enumerate()
            .filter(|(_, n)| among.contains(n))
            .map(|(i, _)| (i, distance(&t, &self.thumbs[i])))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The closest picture, how far off it is, and how far off the next
    /// closest is.
    pub fn best_two(&self, picture: &RgbImage) -> Option<(usize, f32, f32)> {
        let t = thumb(picture, self.size);
        let mut ranked: Vec<(usize, f32)> = self
            .thumbs
            .iter()
            .map(|r| distance(&t, r))
            .enumerate()
            .collect();
        ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (best, d) = *ranked.first()?;
        Some((best, d, ranked.get(1).map_or(f32::MAX, |r| r.1)))
    }

    /// The closest picture, and how far off it is.
    pub fn best(&self, picture: &RgbImage) -> Option<(usize, f32)> {
        let t = thumb(picture, self.size);
        self.thumbs
            .iter()
            .map(|r| distance(&t, r))
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

/// A small copy of a picture, each channel shifted and scaled to mean 0 and
/// spread 1 so brightness and contrast differences don't matter.
fn thumb(image: &RgbImage, size: ThumbSize) -> Thumb {
    let small =
        image::imageops::resize(image, size.w, size.h, image::imageops::FilterType::Triangle);
    let mut out = Vec::with_capacity((size.w * size.rows * 3) as usize);
    for c in 0..3 {
        let values: Vec<f32> = (0..size.rows)
            .flat_map(|y| (0..size.w).map(move |x| (x, y)))
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
