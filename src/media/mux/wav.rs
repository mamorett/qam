use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};

use crate::errors::QamError;

/// Writes 16-bit signed linear PCM samples to a RIFF WAV file.
pub fn write_wav_s16le(
    path: &Path,
    samples: &[i16],
    sample_rate: u32,
    channels: u16,
) -> Result<(), QamError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let file = File::create(path)?;
    let buf_writer = BufWriter::new(file);
    let mut writer = WavWriter::new(buf_writer, spec)
        .map_err(|e| QamError::Media(format!("failed to initialize WAV writer: {e}")))?;

    for &sample in samples {
        writer
            .write_sample(sample)
            .map_err(|e| QamError::Media(format!("failed to write WAV sample: {e}")))?;
    }

    writer
        .finalize()
        .map_err(|e| QamError::Media(format!("failed to finalize WAV: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wav");
        let samples = vec![0i16, 1000, -1000, 2000, -2000];

        write_wav_s16le(&path, &samples, 8000, 1).unwrap();
        assert!(path.exists());

        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 8000);
        assert_eq!(reader.spec().bits_per_sample, 16);
        let read_samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(read_samples, samples);
    }
}
