//! Reads text off the screen with the text recognition built into Windows
//! (Windows.Media.Ocr), in any language installed for it.

use image::RgbImage;

/// The languages Windows can recognise on this computer, like `en-US` or
/// `zh-Hans-CN`.
#[cfg(windows)]
pub fn languages() -> Vec<String> {
    use windows::Media::Ocr::OcrEngine;

    let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(|lang| Some(lang.LanguageTag().ok()?.to_string()))
        .collect()
}

#[cfg(not(windows))]
pub fn languages() -> Vec<String> {
    Vec::new()
}

/// The text in a picture, read as language `lang` (one of [`languages`]).
#[cfg(windows)]
pub fn read_text(image: &RgbImage, lang: &str) -> anyhow::Result<String> {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    use windows::core::HSTRING;

    let (w, h) = image.dimensions();
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for p in image.pixels() {
        let [r, g, b] = p.0;
        bgra.extend_from_slice(&[b, g, r, 255]);
    }
    let writer = DataWriter::new()?;
    writer.WriteBytes(&bgra)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        w as i32,
        h as i32,
        BitmapAlphaMode::Premultiplied,
    )?;
    let engine =
        OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from(lang))?)?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    Ok(result.Text()?.to_string())
}

#[cfg(not(windows))]
pub fn read_text(_image: &RgbImage, _lang: &str) -> anyhow::Result<String> {
    anyhow::bail!("text recognition is only supported on Windows for now")
}
