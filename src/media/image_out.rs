use std::fs::File;
use std::io::Write;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder, RgbImage};

use crate::errors::QamError;

/// Maps a CLI `--quality` value to a JPEG quality percentage (1..=100).
///
/// In camsnap (ffmpeg), `--quality` uses the `q:v` scale where:
/// - 0 is default (~85 in qam)
/// - 1 is best quality (~98)
/// - 31 is worst quality (~20)
pub fn map_quality(q: u8) -> u8 {
    match q {
        0 => 85,
        1..=31 => {
            let val = 98.0 - (q as f32 - 1.0) * (78.0 / 30.0);
            (val.round() as u8).clamp(1, 100)
        }
        _ => 20,
    }
}

pub enum ImageFormat {
    Jpeg,
    Png,
}

pub fn detect_format_from_path(path: &Path) -> ImageFormat {
    if let Some(ext) = path.extension().and_then(|s| s.to_str())
        && ext.eq_ignore_ascii_case("png")
    {
        return ImageFormat::Png;
    }
    ImageFormat::Jpeg
}

/// Encodes an RGB image into the target format buffer.
pub fn encode_image(img: &RgbImage, format: ImageFormat, quality: u8) -> Result<Vec<u8>, QamError> {
    let mut bytes = Vec::new();
    let (width, height) = (img.width(), img.height());

    match format {
        ImageFormat::Jpeg => {
            let mapped_q = map_quality(quality);
            let encoder = JpegEncoder::new_with_quality(&mut bytes, mapped_q);
            encoder
                .write_image(img.as_raw(), width, height, ColorType::Rgb8.into())
                .map_err(|e| QamError::Media(format!("JPEG encoding failed: {e}")))?;
        }
        ImageFormat::Png => {
            let encoder = PngEncoder::new(&mut bytes);
            encoder
                .write_image(img.as_raw(), width, height, ColorType::Rgb8.into())
                .map_err(|e| QamError::Media(format!("PNG encoding failed: {e}")))?;
        }
    }

    Ok(bytes)
}

/// Saves an encoded image buffer to `out_path`.
///
/// If `out_path` is `"-"`, writes raw bytes to stdout.
/// If `out_path` exists and `overwrite` is `false`, returns an error matching Go:
///   `output file exists: <path>`
/// Otherwise, writes atomically via a temporary file `<out_path>.tmp.<pid>`.
pub fn write_image_file(bytes: &[u8], out_path: &str, overwrite: bool) -> Result<(), QamError> {
    if out_path == "-" {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(bytes)?;
        stdout.flush()?;
        return Ok(());
    }

    let dest = Path::new(out_path);
    if dest.exists() && !overwrite {
        return Err(QamError::Media(format!("output file exists: {out_path}")));
    }

    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = format!("{}.tmp.{}", out_path, std::process::id());
    let tmp = Path::new(&tmp_path);

    let mut file = File::create(tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    std::fs::rename(tmp, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_mapping() {
        assert_eq!(map_quality(0), 85);
        assert_eq!(map_quality(1), 98);
        assert_eq!(map_quality(31), 20);
        assert_eq!(map_quality(16), 59);
    }

    #[test]
    fn test_encode_and_write() {
        let img = RgbImage::new(10, 10);
        let bytes = encode_image(&img, ImageFormat::Jpeg, 0).unwrap();
        assert!(!bytes.is_empty());
        // Verify JPEG magic bytes FF D8
        assert_eq!(&bytes[0..2], &[0xFF, 0xD8]);

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("test.jpg");
        let target_str = target.to_str().unwrap();

        write_image_file(&bytes, target_str, true).unwrap();
        assert!(target.exists());

        // Overwrite false fails
        let err = write_image_file(&bytes, target_str, false).unwrap_err();
        assert!(err.to_string().contains("output file exists:"));
    }
}
