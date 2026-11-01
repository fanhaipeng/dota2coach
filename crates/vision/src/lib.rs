//! Reads the Dota 2 screen, for what GSI doesn't report: enemies on the
//! minimap, who is dead on the top bar.
//!
//! It only looks at what is already on the player's screen. It never sends
//! input to the game or reads its memory.
//!
//! Step one is capture: grab the screen Dota is on and save frames during a
//! match, so the readers can be built and tested against real games.

use std::path::{Path, PathBuf};

use image::RgbaImage;

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

/// Saves a frame every few seconds of game time to
/// `<dir>/<match id>/<clock>.jpg`, so frames line up with the GSI recording
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

    /// Where the frame for this moment should go, or `None` if one was saved
    /// too recently. Marks the moment as taken.
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

/// `<dir>/<match id>/<clock as 5 digits>.jpg`, so frames sort by time.
pub fn frame_path(dir: &Path, match_id: &str, clock: i64) -> PathBuf {
    dir.join(match_id).join(format!("{clock:05}.jpg"))
}

/// Writes a frame as JPEG, creating its folder.
pub fn save_frame(image: &RgbaImage, path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let rgb = image::DynamicImage::ImageRgba8(image.clone()).into_rgb8();
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&rgb)?;
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
            Some(PathBuf::from("frames").join("7").join("00030.jpg"))
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
    fn writes_a_jpeg() {
        let dir = std::env::temp_dir().join(format!("dota2coach-frames-{}", std::process::id()));
        let path = frame_path(&dir, "7", 30);
        save_frame(
            &RgbaImage::from_pixel(64, 36, image::Rgba([200, 30, 30, 255])),
            &path,
        )
        .unwrap();
        let back = image::open(&path).unwrap();
        assert_eq!((back.width(), back.height()), (64, 36));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
