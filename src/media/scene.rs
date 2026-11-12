/// Detects scene changes based on Mean Absolute Frame Difference (MAFD)
/// matching FFmpeg `select='gt(scene,T)'`.
#[derive(Debug, Default)]
pub struct SceneDetector {
    prev_y: Option<Vec<u8>>,
    prev_width: usize,
    prev_height: usize,
    prev_mafd: f64,
}

impl SceneDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a new frame's luma (Y) plane and returns the scene change score in [0.0, 1.0].
    pub fn feed(&mut self, y: &[u8], width: usize, height: usize) -> f64 {
        if self.prev_width != width || self.prev_height != height {
            self.prev_width = width;
            self.prev_height = height;
            self.prev_y = Some(y.to_vec());
            return 0.0;
        }

        let Some(prev) = self.prev_y.as_deref() else {
            self.prev_y = Some(y.to_vec());
            return 0.0;
        };

        if prev.len() != y.len() || y.is_empty() {
            self.prev_y = Some(y.to_vec());
            return 0.0;
        }

        let sum: u64 = prev
            .iter()
            .zip(y.iter())
            .map(|(&a, &b)| (a as i32 - b as i32).unsigned_abs() as u64)
            .sum();

        let mafd = sum as f64 / y.len() as f64;
        let diff = (mafd - self.prev_mafd).abs();
        let score = (mafd.min(diff) / 100.0).clamp(0.0, 1.0);
        self.prev_mafd = mafd;
        if let Some(buf) = self.prev_y.as_mut() {
            buf.copy_from_slice(y);
        }
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct SceneTransitionRef {
        #[allow(dead_code)]
        index: usize,
        lavfi_scene_score: f64,
    }

    #[derive(Deserialize)]
    struct SceneReferenceFixture {
        width: usize,
        height: usize,
        transitions: Vec<SceneTransitionRef>,
    }

    fn generate_synthetic_frames(width: usize, height: usize) -> Vec<Vec<u8>> {
        let y_len = width * height;
        let mut out = Vec::new();

        // 0: solid_black (Y=0)
        out.push(vec![0u8; y_len]);

        // 1: gray_50 (Y=128)
        out.push(vec![128u8; y_len]);

        // 2: checkerboard (16x16 blocks of 0 and 255)
        let mut f2 = vec![0u8; y_len];
        for y in 0..height {
            for x in 0..width {
                let block = ((x / 16) + (y / 16)) % 2;
                f2[y * width + x] = if block == 0 { 0 } else { 255 };
            }
        }
        out.push(f2);

        // 3: horizontal_gradient (Y = x * 255 / (width - 1))
        let mut f3 = vec![0u8; y_len];
        for y in 0..height {
            for x in 0..width {
                f3[y * width + x] = ((x * 255) / (width - 1)) as u8;
            }
        }
        out.push(f3);

        // 4: solid_white (Y=255)
        out.push(vec![255u8; y_len]);

        out
    }

    #[test]
    fn test_scene_detector_against_ffmpeg_fixture() {
        let fixture_str = include_str!("../../tests/fixtures/scene_reference.json");
        let fixture: SceneReferenceFixture =
            serde_json::from_str(fixture_str).expect("parse scene_reference.json");

        let frames = generate_synthetic_frames(fixture.width, fixture.height);
        assert_eq!(frames.len(), fixture.transitions.len());

        let mut detector = SceneDetector::new();
        for (i, frame) in frames.iter().enumerate() {
            let score = detector.feed(frame, fixture.width, fixture.height);
            let expected = fixture.transitions[i].lavfi_scene_score;
            let diff = (score - expected).abs();
            assert!(
                diff < 1e-5,
                "transition {i}: got {score}, expected {expected}, diff={diff}"
            );
        }
    }
}
