//! Reads the Dota 2 screen, for what GSI doesn't report: enemies on the
//! minimap, who is dead on the top bar.
//!
//! It only looks at what is already on the player's screen. It never sends
//! input to the game or reads its memory.
//!
//! Step one is capture: grab the screen Dota is on and save frames during a
//! match, so the readers can be built and tested against real games. The
//! readers are [`topbar`], who is dead and their respawn timers, and
//! [`minimap`], where the heroes you can see are.

use std::path::{Path, PathBuf};

use image::RgbaImage;

mod gallery;
pub mod heroes;
pub mod items;
pub mod minimap;
pub mod ocr;
pub mod topbar;

/// The screen Dota 2 is on, captured once.
///
/// Dota must run in windowed or desktop-friendly fullscreen mode; exclusive
/// fullscreen may capture as black.
#[cfg(windows)]
pub fn capture_dota() -> anyhow::Result<RgbaImage> {
    use anyhow::Context;

    let window = xcap::Window::all()?
        .into_iter()
        .find(|w| w.title().is_ok_and(|t| t == "Dota 2") && !w.is_minimized().unwrap_or(false))
        .context("no Dota 2 window")?;
    let monitor = window.current_monitor()?;
    Ok(monitor.capture_image()?)
}

#[cfg(not(windows))]
pub fn capture_dota() -> anyhow::Result<RgbaImage> {
    anyhow::bail!("screen capture is only supported on Windows for now")
}

/// The monitor Dota 2 is on, kept between reads so live reading only
/// captures the small parts of the screen it needs.
#[cfg(windows)]
pub struct DotaScreen {
    monitor: xcap::Monitor,
    width: u32,
    height: u32,
}

#[cfg(windows)]
impl DotaScreen {
    pub fn find() -> anyhow::Result<Self> {
        use anyhow::Context;

        let window = xcap::Window::all()?
            .into_iter()
            .find(|w| w.title().is_ok_and(|t| t == "Dota 2") && !w.is_minimized().unwrap_or(false))
            .context("no Dota 2 window")?;
        let monitor = window.current_monitor()?;
        let (width, height) = (monitor.width()?, monitor.height()?);
        Ok(Self {
            monitor,
            width,
            height,
        })
    }

    /// The top bar, ready for [`topbar::read`].
    pub fn top_bar(&self) -> anyhow::Result<image::RgbImage> {
        let r = top_bar_area(self.width, self.height);
        let part = self.monitor.capture_region(r.x, r.y, r.width, r.height)?;
        Ok(image::DynamicImage::ImageRgba8(part).into_rgb8())
    }

    /// The ability and item panel, ready for [`items::Icons::inventory`].
    pub fn panel(&self) -> anyhow::Result<image::RgbImage> {
        let r = bottom_panel_area(self.width, self.height);
        let part = self.monitor.capture_region(r.x, r.y, r.width, r.height)?;
        Ok(image::DynamicImage::ImageRgba8(part).into_rgb8())
    }

    /// The first quick-buy slot, ready for [`quickbuy_set`].
    pub fn quickbuy(&self) -> anyhow::Result<image::RgbImage> {
        let r = quickbuy_area(self.width, self.height);
        let part = self.monitor.capture_region(r.x, r.y, r.width, r.height)?;
        Ok(image::DynamicImage::ImageRgba8(part).into_rgb8())
    }

    /// The minimap, ready for [`minimap::Reader::read`].
    pub fn minimap(&self) -> anyhow::Result<image::RgbImage> {
        let r = minimap_area(self.width, self.height);
        let part = self.monitor.capture_region(r.x, r.y, r.width, r.height)?;
        Ok(image::DynamicImage::ImageRgba8(part).into_rgb8())
    }
}

#[cfg(not(windows))]
pub struct DotaScreen;

#[cfg(not(windows))]
impl DotaScreen {
    pub fn find() -> anyhow::Result<Self> {
        anyhow::bail!("screen reading is only supported on Windows for now")
    }

    pub fn top_bar(&self) -> anyhow::Result<image::RgbImage> {
        anyhow::bail!("screen reading is only supported on Windows for now")
    }

    pub fn minimap(&self) -> anyhow::Result<image::RgbImage> {
        anyhow::bail!("screen reading is only supported on Windows for now")
    }

    pub fn panel(&self) -> anyhow::Result<image::RgbImage> {
        anyhow::bail!("screen reading is only supported on Windows for now")
    }

    pub fn quickbuy(&self) -> anyhow::Result<image::RgbImage> {
        anyhow::bail!("screen reading is only supported on Windows for now")
    }
}

/// Saves a frame every few seconds of game time under
/// `<dir>/<match id>/<clock>-*.jpg`, so frames line up with the GSI recording
/// of the same match.
#[derive(Debug)]
pub struct FrameSaver {
    dir: PathBuf,
    every_secs: i64,
    last: Option<(String, i64)>,
}

impl FrameSaver {
    pub fn new(dir: impl Into<PathBuf>, every_secs: i64) -> Self {
        Self {
            dir: dir.into(),
            every_secs: every_secs.max(1),
            last: None,
        }
    }

    /// Where the frame for this moment should go (see [`save_frame`]), or
    /// `None` if one was saved too recently. Marks the moment as taken.
    pub fn due(&mut self, match_id: &str, clock: i64) -> Option<PathBuf> {
        if match_id.is_empty() || clock < 0 {
            return None;
        }
        if let Some((id, at)) = &self.last
            && id == match_id
            && clock >= *at
            && clock - at < self.every_secs
        {
            return None;
        }
        self.last = Some((match_id.to_owned(), clock));
        Some(frame_path(&self.dir, match_id, clock))
    }
}

/// `<dir>/<match id>/<clock as 5 digits>`, so frames sort by time.
pub fn frame_path(dir: &Path, match_id: &str, clock: i64) -> PathBuf {
    dir.join(match_id).join(format!("{clock:05}"))
}

/// Crops of taller screens are scaled down to this height's detail.
const DETAIL_HEIGHT: u32 = 1080;
/// Width of the small copy of the whole screen.
const SCREEN_WIDTH: u32 = 960;

/// A part of the screen, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Where the minimap is with the default HUD: a square in the bottom-left
/// corner, sized by screen height. Measured at 4K as 495 pixels, plus a little
/// margin.
pub fn minimap_area(width: u32, height: u32) -> Rect {
    let side = (height * 24 / 100).min(width);
    Rect {
        x: 0,
        y: height - side,
        width: side,
        height: side,
    }
}

/// Where the hero portraits, clock and respawn timers are: a band along the
/// top, centred, sized by screen height so it also fits ultrawide screens.
/// Measured at 4K as 1710 by 135 pixels.
pub fn top_bar_area(width: u32, height: u32) -> Rect {
    let band = (height * 82 / 100).min(width);
    Rect {
        x: (width - band) / 2,
        y: 0,
        width: band,
        height: height * 65 / 1000,
    }
}

/// The first slot of the quick-buy tray, above the gold count in the bottom
/// right corner, sized by screen height.
pub fn quickbuy_area(width: u32, height: u32) -> Rect {
    let x = width.saturating_sub(height * 265 / 1000);
    let y = height - height * 93 / 1000;
    Rect {
        x,
        y,
        width: height * 39 / 1000,
        height: height * 28 / 1000,
    }
}

/// Mean colourfulness above which the quick-buy slot holds an item; empty,
/// it shows a grey hint.
const QUICKBUY_COLOUR: u32 = 25;

/// Whether an item is queued for quick-buy, from a crop of
/// [`quickbuy_area`] at any size.
pub fn quickbuy_set(slot: &image::RgbImage) -> bool {
    let n = (slot.width() * slot.height()).max(1);
    let colour: u32 = slot
        .pixels()
        .map(|p| {
            let [r, g, b] = p.0;
            u32::from(r.max(g).max(b) - r.min(g).min(b))
        })
        .sum();
    colour / n >= QUICKBUY_COLOUR
}

/// Where the hero's portrait, abilities and item slots are: a band along the
/// bottom, centred, sized by screen height.
pub fn bottom_panel_area(width: u32, height: u32) -> Rect {
    let band = height.min(width);
    let tall = height * 18 / 100;
    Rect {
        x: (width - band) / 2,
        y: height - tall,
        width: band,
        height: tall,
    }
}

/// Saves what the readers need from one screenshot, next to `stem`:
/// `<stem>-minimap.jpg`, `<stem>-topbar.jpg` and `<stem>-items.jpg` (the
/// bottom panel with abilities and items) at up to 1080p detail, and
/// `<stem>-screen.jpg`, the whole screen scaled down to 960 pixels wide for
/// context.
pub fn save_frame(image: &RgbaImage, stem: &Path) -> anyhow::Result<()> {
    if let Some(dir) = stem.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (w, h) = image.dimensions();
    let rgb = image::DynamicImage::ImageRgba8(image.clone()).into_rgb8();
    // Above 1080p, scale the crops down to 1080p detail: enough to read, and
    // a 4K crop would be four times the size.
    let crop = |r: Rect| {
        let part = image::imageops::crop_imm(&rgb, r.x, r.y, r.width, r.height).to_image();
        if h <= DETAIL_HEIGHT {
            return part;
        }
        image::imageops::resize(
            &part,
            r.width * DETAIL_HEIGHT / h,
            r.height * DETAIL_HEIGHT / h,
            image::imageops::FilterType::CatmullRom,
        )
    };
    write_jpeg(&crop(minimap_area(w, h)), &suffixed(stem, "minimap"), 90)?;
    write_jpeg(&crop(top_bar_area(w, h)), &suffixed(stem, "topbar"), 90)?;
    write_jpeg(&crop(bottom_panel_area(w, h)), &suffixed(stem, "items"), 90)?;
    let small = if w > SCREEN_WIDTH {
        image::imageops::resize(
            &rgb,
            SCREEN_WIDTH,
            h * SCREEN_WIDTH / w,
            image::imageops::FilterType::Triangle,
        )
    } else {
        rgb
    };
    write_jpeg(&small, &suffixed(stem, "screen"), 80)
}

fn suffixed(stem: &Path, part: &str) -> PathBuf {
    let mut name = stem.as_os_str().to_owned();
    name.push(format!("-{part}.jpg"));
    PathBuf::from(name)
}

fn write_jpeg(image: &image::RgbImage, path: &Path, quality: u8) -> anyhow::Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(image)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_one_frame_per_interval_per_match() {
        let mut saver = FrameSaver::new("frames", 10);
        assert_eq!(saver.due("", 30), None, "no match yet");
        assert_eq!(saver.due("7", -20), None, "before the horn");
        assert_eq!(
            saver.due("7", 30),
            Some(PathBuf::from("frames").join("7").join("00030"))
        );
        assert_eq!(saver.due("7", 35), None);
        assert!(saver.due("7", 40).is_some());
        assert!(saver.due("8", 41).is_some(), "a new match starts over");
        assert!(
            saver.due("8", 5).is_some(),
            "clock went back, e.g. a replay"
        );
    }

    #[test]
    fn finds_the_hud_areas() {
        assert_eq!(
            minimap_area(1920, 1080),
            Rect {
                x: 0,
                y: 821,
                width: 259,
                height: 259
            }
        );
        assert_eq!(
            top_bar_area(1920, 1080),
            Rect {
                x: 517,
                y: 0,
                width: 885,
                height: 70
            }
        );
        assert_eq!(
            bottom_panel_area(1920, 1080),
            Rect {
                x: 420,
                y: 886,
                width: 1080,
                height: 194
            }
        );
        // Ultrawide: same size as 1440p, centred.
        assert_eq!(top_bar_area(3440, 1440).x, (3440 - 1180) / 2);
    }

    #[test]
    fn sees_an_item_queued_for_quick_buy() {
        let r = quickbuy_area(3840, 2160);
        assert_eq!((r.x, r.y, r.width, r.height), (3268, 1960, 84, 60));
        let hint = image::RgbImage::from_pixel(84, 60, image::Rgb([120, 120, 125]));
        assert!(!quickbuy_set(&hint));
        let blade = image::RgbImage::from_pixel(84, 60, image::Rgb([60, 170, 70]));
        assert!(quickbuy_set(&blade));
    }

    #[test]
    fn saves_the_hud_areas_and_a_small_screen() {
        let dir = std::env::temp_dir().join(format!("dota2coach-frames-{}", std::process::id()));
        let stem = frame_path(&dir, "7", 30);
        let screen = RgbaImage::from_pixel(1920, 1080, image::Rgba([200, 30, 30, 255]));
        save_frame(&screen, &stem).unwrap();
        let size = |part: &str| {
            let img = image::open(dir.join("7").join(format!("00030-{part}.jpg"))).unwrap();
            (img.width(), img.height())
        };
        assert_eq!(size("minimap"), (259, 259));
        assert_eq!(size("topbar"), (885, 70));
        assert_eq!(size("items"), (1080, 194));
        assert_eq!(size("screen"), (960, 540));

        // 4K crops come out the size of 1080p ones.
        let screen = RgbaImage::from_pixel(3840, 2160, image::Rgba([30, 200, 30, 255]));
        save_frame(&screen, &stem).unwrap();
        assert_eq!(size("minimap"), (259, 259));
        assert_eq!(size("topbar"), (885, 70));
        assert_eq!(size("items"), (1080, 194));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
