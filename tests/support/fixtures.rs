#![allow(dead_code)]

use openh264::encoder::Encoder as H264Encoder;
use openh264::formats::YUVBuffer;

/// Generates `num_frames` of synthetic H.264 Annex-B frames (320x240, 10 fps).
/// The first frame is guaranteed to be an IDR keyframe containing SPS/PPS.
pub fn generate_synthetic_h264(num_frames: usize, width: usize, height: usize) -> Vec<Vec<u8>> {
    let mut enc = H264Encoder::new().expect("failed to create H264Encoder");
    let mut frames = Vec::with_capacity(num_frames);

    let y_len = width * height;
    let uv_len = (width / 2) * (height / 2);
    let total_len = y_len + 2 * uv_len;

    for i in 0..num_frames {
        let mut raw = vec![128u8; total_len];
        // Gray background with moving square
        let square_x = (i * 15) % (width - 40);
        let square_y = (i * 10) % (height - 40);

        for y in square_y..square_y + 40 {
            for x in square_x..square_x + 40 {
                raw[y * width + x] = 240;
            }
        }

        let yuv = YUVBuffer::from_vec(raw, width, height);

        let bitstream = enc.encode(&yuv).expect("encode frame").to_vec();
        frames.push(bitstream);
    }

    frames
}

/// Generates G.711 A-law (PCMA) audio from a 1 kHz sine wave at 8000 Hz sample rate.
pub fn generate_g711_pcma(num_samples: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let t = i as f64 / 8000.0;
        let sample = (2.0 * std::f64::consts::PI * 1000.0 * t).sin();
        let pcm_s16 = (sample * 32767.0) as i16;
        out.push(linear_to_alaw(pcm_s16));
    }
    out
}

/// Generates G.711 mu-law (PCMU) audio from a 1 kHz sine wave at 8000 Hz sample rate.
pub fn generate_g711_pcmu(num_samples: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let t = i as f64 / 8000.0;
        let sample = (2.0 * std::f64::consts::PI * 1000.0 * t).sin();
        let pcm_s16 = (sample * 32767.0) as i16;
        out.push(linear_to_ulaw(pcm_s16));
    }
    out
}

/// Standard 7-byte raw silent AAC-LC frame.
pub fn silent_aac_frame() -> &'static [u8] {
    &[0x21, 0x10, 0x05, 0x00, 0xa0, 0x18, 0x00]
}

/// Loads the test HEVC / H.265 bitstream from fixtures.
pub fn sample_hevc_bytes() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("test_hevc.h265");
    std::fs::read(&path).expect("read test_hevc.h265 fixture")
}

// G.711 ITU-T G.711 reference conversion helpers
fn linear_to_alaw(pcm_val: i16) -> u8 {
    let mut pcm = pcm_val;
    let mask = if pcm >= 0 {
        0xD5
    } else {
        pcm = -pcm - 1;
        0x55
    };

    let seg = match pcm >> 8 {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        4..=7 => 3,
        8..=15 => 4,
        16..=31 => 5,
        32..=63 => 6,
        _ => 7,
    };

    let aval = if seg >= 2 {
        ((pcm >> (seg + 3)) & 0x0F) | (seg << 4)
    } else {
        ((pcm >> 4) & 0x0F) | (seg << 4)
    };

    (aval as u8) ^ mask
}

fn linear_to_ulaw(pcm_val: i16) -> u8 {
    const BIAS: i16 = 0x84;
    const CLIP: i16 = 32635;

    let mut pcm = pcm_val;
    let sign = (pcm >> 8) & 0x80;
    if sign != 0 {
        pcm = -pcm;
    }
    if pcm > CLIP {
        pcm = CLIP;
    }
    pcm += BIAS;

    let seg = match pcm >> 7 {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        4..=7 => 3,
        8..=15 => 4,
        16..=31 => 5,
        32..=63 => 6,
        _ => 7,
    };

    let uval = sign | (seg << 4) | ((pcm >> (seg + 3)) & 0x0F);
    !(uval as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthetic_h264_generation() {
        let frames = generate_synthetic_h264(3, 320, 240);
        assert_eq!(frames.len(), 3);
        assert!(!frames[0].is_empty());
        // Verify start code
        assert_eq!(&frames[0][..4], &[0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn test_g711_generation() {
        let pcma = generate_g711_pcma(160);
        assert_eq!(pcma.len(), 160);
        let pcmu = generate_g711_pcmu(160);
        assert_eq!(pcmu.len(), 160);
    }
}
