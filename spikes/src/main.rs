use std::num::NonZeroU16;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use openh264::decoder::Decoder as H264Decoder;
use openh264::encoder::Encoder as H264Encoder;
use openh264::formats::{YUVBuffer, YUVSource};
use retina::client::{AuthPolicy, Credentials, SessionOptions};
use retina::codec::Depacketizer;
use serde::Serialize;

#[derive(Serialize)]
struct SceneTransitionRef {
    index: usize,
    from: String,
    to: String,
    lavfi_scene_score: f64,
    rust_scene_score: f64,
    abs_diff: f64,
}

#[derive(Serialize)]
struct SceneReferenceFixture {
    width: usize,
    height: usize,
    frames: Vec<String>,
    transitions: Vec<SceneTransitionRef>,
}

/// Deterministic synthetic YUV420p frame generator (5 frames: black, 50% gray, checkerboard, gradient, white).
fn generate_synthetic_frames(width: usize, height: usize) -> Vec<(String, Vec<u8>)> {
    let y_len = width * height;
    let uv_len = (width / 2) * (height / 2);
    let total_len = y_len + 2 * uv_len;

    let mut out = Vec::new();

    // 0: solid_black (Y=0)
    let mut f0 = vec![128u8; total_len];
    f0[..y_len].fill(0);
    out.push(("solid_black".to_string(), f0));

    // 1: gray_50 (Y=128)
    let mut f1 = vec![128u8; total_len];
    f1[..y_len].fill(128);
    out.push(("gray_50".to_string(), f1));

    // 2: checkerboard (16x16 blocks of 0 and 255)
    let mut f2 = vec![128u8; total_len];
    for y in 0..height {
        for x in 0..width {
            let block = ((x / 16) + (y / 16)) % 2;
            f2[y * width + x] = if block == 0 { 0 } else { 255 };
        }
    }
    out.push(("checkerboard".to_string(), f2));

    // 3: horizontal_gradient (Y = x * 255 / (width - 1))
    let mut f3 = vec![128u8; total_len];
    for y in 0..height {
        for x in 0..width {
            f3[y * width + x] = ((x * 255) / (width - 1)) as u8;
        }
    }
    out.push(("horizontal_gradient".to_string(), f3));

    // 4: solid_white (Y=255)
    let mut f4 = vec![128u8; total_len];
    f4[..y_len].fill(255);
    out.push(("solid_white".to_string(), f4));

    out
}

struct SceneDetector {
    prev_y: Option<Vec<u8>>,
    prev_mafd: f64,
}

impl SceneDetector {
    fn new() -> Self {
        Self {
            prev_y: None,
            prev_mafd: 0.0,
        }
    }

    fn feed(&mut self, y: &[u8]) -> f64 {
        let Some(prev) = self.prev_y.as_deref() else {
            self.prev_y = Some(y.to_vec());
            return 0.0;
        };
        let sum: u64 = prev
            .iter()
            .zip(y.iter())
            .map(|(&a, &b)| (a as i32 - b as i32).unsigned_abs() as u64)
            .sum();
        let mafd = sum as f64 / y.len() as f64;
        let diff = (mafd - self.prev_mafd).abs();
        let score = ((mafd.min(diff)) / 100.0).clamp(0.0, 1.0);
        self.prev_mafd = mafd;
        self.prev_y.as_mut().unwrap().copy_from_slice(y);
        score
    }
}

#[tokio::main]
async fn main() {
    println!("=== Phase 0 Spikes ===");

    // 1. Vendor retina audit & feature checks
    let opts = SessionOptions::default()
        .creds(Some(Credentials {
            username: "admin".into(),
            password: "secret".into(),
        }))
        .auth_policy(AuthPolicy::Digest);
    drop(opts);

    let opus_depack = Depacketizer::new("audio", "opus", 48000, NonZeroU16::new(2), None);
    assert!(opus_depack.is_ok(), "Opus depacketizer must be supported");
    println!("[OK] vendor/retina: AuthPolicy + audio/opus depacketizer");

    // Test rtsps:// TLS loopback connection
    test_rtsps_loopback().await;
    println!("[OK] vendor/retina: rtsps:// TLS loopback handshake & DESCRIBE");

    // 2. OpenH264 encode + decode + 1080p@15fps benchmark
    let mut enc = H264Encoder::new().expect("H264Encoder::new");
    let mut dec = H264Decoder::new().expect("H264Decoder::new");

    let yuv_64 = YUVBuffer::new(64, 64);
    let bs_64 = enc.encode(&yuv_64).expect("encode 64x64").to_vec();
    let mut decoded_64 = None;
    for nal in openh264::nal_units(&bs_64) {
        if let Some(frame) = dec.decode(nal).expect("decode nal") {
            decoded_64 = Some(frame.dimensions());
        }
    }
    assert_eq!(decoded_64, Some((64, 64)));

    // 1080p@15fps benchmark for OpenH264
    let enc_cfg = openh264::encoder::EncoderConfig::new()
        .skip_frames(false)
        .bitrate(openh264::encoder::BitRate::from_bps(4_000_000));
    let mut enc_1080 = H264Encoder::with_api_config(openh264::OpenH264API::from_source(), enc_cfg)
        .expect("H264Encoder 1080p");
    let mut frames_1080_h264 = Vec::new();
    for i in 0..15 {
        let mut raw = vec![128u8; 1920 * 1080 * 3 / 2];
        for (idx, b) in raw[..1920 * 1080].iter_mut().enumerate() {
            *b = ((idx + i * 17) & 0xFF) as u8;
        }
        let yuv = YUVBuffer::from_vec(raw, 1920, 1080);
        frames_1080_h264.push(enc_1080.encode(&yuv).expect("encode 1080p").to_vec());
    }
    let mut dec_1080 = H264Decoder::new().expect("H264Decoder 1080p");
    let t0 = Instant::now();
    let mut decoded_count = 0;
    for pkt in &frames_1080_h264 {
        for nal in openh264::nal_units(pkt) {
            if let Some(f) = dec_1080.decode(nal).expect("decode 1080p") {
                assert_eq!(f.dimensions(), (1920, 1080));
                decoded_count += 1;
            }
        }
    }
    let h264_elapsed = t0.elapsed();
    let h264_ms_per_frame = h264_elapsed.as_secs_f64() * 1000.0 / decoded_count as f64;
    let h264_fps = decoded_count as f64 / h264_elapsed.as_secs_f64();
    println!(
        "[OK] openh264 1080p decode: {} frames in {:.2?} ({:.2} ms/frame, {:.1} fps)",
        decoded_count, h264_elapsed, h264_ms_per_frame, h264_fps
    );

    // 3. Generate H.265 fixtures & benchmark rust_h265 vs rusty_h265
    std::fs::create_dir_all("../tests/fixtures").expect("mkdir tests/fixtures");
    let hevc_fixture_path = "../tests/fixtures/test_hevc.h265";
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=15:duration=0.2",
            "-frames:v",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx265",
            "-profile:v",
            "main",
            "-x265-params",
            "keyint=1:bframes=0:log-level=none",
            "-f",
            "hevc",
            hevc_fixture_path,
        ])
        .status()
        .expect("run offline ffmpeg for test_hevc.h265");
    assert!(status.success(), "ffmpeg failed to generate test_hevc.h265");
    let hevc_64_bytes = std::fs::read(hevc_fixture_path).expect("read test_hevc.h265");
    println!(
        "[OK] Generated {} ({} bytes)",
        hevc_fixture_path,
        hevc_64_bytes.len()
    );

    // Also generate 15 frames of 1080p H.265 for benchmarking
    let tmp_1080_hevc = "/tmp/qam_spike_1080p.h265";
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=1920x1080:rate=15:duration=1",
            "-frames:v",
            "15",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx265",
            "-preset",
            "ultrafast",
            "-x265-params",
            "bframes=0:log-level=none",
            "-f",
            "hevc",
            tmp_1080_hevc,
        ])
        .status()
        .expect("run offline ffmpeg for 1080p hevc");
    assert!(status.success());
    let hevc_1080_bytes = std::fs::read(tmp_1080_hevc).expect("read 1080p hevc");
    let _ = std::fs::remove_file(tmp_1080_hevc);

    // Benchmark rust_h265
    {
        let mut dec = rust_h265::Decoder::new().with_display_order();
        let mut small_frames = Vec::new();
        for nal in rust_h265::parse_annex_b(&hevc_64_bytes) {
            dec.decode_nal(&nal).expect("rust_h265 decode 64x64");
            while let Some(f) = dec.next_output() {
                small_frames.push((f.width, f.height));
            }
        }
        while let Some(f) = dec.flush() {
            small_frames.push((f.width, f.height));
        }
        assert_eq!(small_frames, vec![(64, 64), (64, 64)]);

        let mut dec_1080 = rust_h265::Decoder::new().with_display_order();
        let t0 = Instant::now();
        let nals = rust_h265::parse_annex_b(&hevc_1080_bytes);
        let mut count = 0;
        for nal in &nals {
            dec_1080.decode_nal(nal).expect("rust_h265 decode 1080p");
            while let Some(f) = dec_1080.next_output() {
                assert_eq!((f.width, f.height), (1920, 1080));
                count += 1;
            }
        }
        while let Some(f) = dec_1080.flush() {
            assert_eq!((f.width, f.height), (1920, 1080));
            count += 1;
        }
        let elapsed = t0.elapsed();
        println!(
            "[OK] rust_h265 1080p decode: {} frames in {:.2?} ({:.2} ms/frame, {:.1} fps)",
            count,
            elapsed,
            elapsed.as_secs_f64() * 1000.0 / count as f64,
            count as f64 / elapsed.as_secs_f64()
        );
    }

    // Benchmark rusty_h265
    {
        let mut dec_1080 = rusty_h265::Decoder::new();
        let t0 = Instant::now();
        dec_1080
            .push_annexb(&hevc_1080_bytes, None)
            .expect("rusty_h265 1080p");
        dec_1080.flush();
        let mut count = 0;
        while let Ok(f) = dec_1080.next_frame() {
            assert_eq!((f.width, f.height), (1920, 1080));
            count += 1;
        }
        let elapsed = t0.elapsed();
        println!(
            "[OK] rusty_h265 1080p decode: {} frames in {:.2?} ({:.2} ms/frame, {:.1} fps)",
            count,
            elapsed,
            elapsed.as_secs_f64() * 1000.0 / count as f64,
            count as f64 / elapsed.as_secs_f64()
        );
    }

    // 4. Generate tests/fixtures/scene_reference.json via offline ffmpeg
    let width = 320;
    let height = 180;
    let synth = generate_synthetic_frames(width, height);
    let raw_yuv_path = "/tmp/qam_spike_scene.yuv";
    let mut raw_bytes = Vec::new();
    for (_, buf) in &synth {
        raw_bytes.extend_from_slice(buf);
    }
    std::fs::write(raw_yuv_path, &raw_bytes).expect("write raw yuv");

    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "yuv420p",
            "-s",
            &format!("{width}x{height}"),
            "-r",
            "1",
            "-i",
            raw_yuv_path,
            "-vf",
            "select='gte(scene\\,0)',metadata=print:file=-",
            "-f",
            "null",
            "-",
        ])
        .output()
        .expect("run ffmpeg scene filter");
    let _ = std::fs::remove_file(raw_yuv_path);
    assert!(output.status.success(), "ffmpeg scene filter failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lavfi_scores = Vec::new();
    for line in stdout.lines() {
        if let Some(val) = line.strip_prefix("lavfi.scene_score=") {
            lavfi_scores.push(val.trim().parse::<f64>().expect("parse scene_score"));
        }
    }
    assert_eq!(lavfi_scores.len(), synth.len());

    let mut detector = SceneDetector::new();
    let y_len = width * height;
    let mut transitions = Vec::new();
    for (i, (name, buf)) in synth.iter().enumerate() {
        let rust_score = detector.feed(&buf[..y_len]);
        let lavfi_score = lavfi_scores[i];
        let abs_diff = (rust_score - lavfi_score).abs();
        assert!(
            abs_diff < 1e-5,
            "scene score mismatch at frame {i}: rust={rust_score} vs lavfi={lavfi_score}"
        );
        let from = if i == 0 {
            "initial".to_string()
        } else {
            synth[i - 1].0.clone()
        };
        transitions.push(SceneTransitionRef {
            index: i,
            from,
            to: name.clone(),
            lavfi_scene_score: lavfi_score,
            rust_scene_score: rust_score,
            abs_diff,
        });
    }

    let fixture = SceneReferenceFixture {
        width,
        height,
        frames: synth.iter().map(|(n, _)| n.clone()).collect(),
        transitions,
    };
    let json = serde_json::to_string_pretty(&fixture).expect("serialize json");
    std::fs::write(
        "../tests/fixtures/scene_reference.json",
        format!("{json}\n"),
    )
    .expect("write scene_reference.json");
    println!("[OK] Generated ../tests/fixtures/scene_reference.json (max diff < 1e-6)");
}

async fn test_rtsps_loopback() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let cert_der = rustls::pki_types::CertificateDer::from(cert.cert);
    let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(
        rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()),
    );
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let server_cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_cfg));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut tls = acceptor.accept(stream).await.unwrap();
        let mut buf = [0u8; 1024];
        let n = tls.read(&mut buf).await.unwrap();
        let req = String::from_utf8_lossy(&buf[..n]);
        assert!(req.starts_with("DESCRIBE rtsps://"));
        let sdp = "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=Test\r\nt=0 0\r\nm=video 0 RTP/AVP 96\r\na=rtpmap:96 H264/90000\r\na=control:trackID=0\r\n";
        let resp = format!(
            "RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Type: application/sdp\r\nContent-Length: {}\r\n\r\n{}",
            sdp.len(),
            sdp
        );
        tls.write_all(resp.as_bytes()).await.unwrap();
        tls.flush().await.unwrap();
    });

    let url = url::Url::parse(&format!("rtsps://127.0.0.1:{}/live", addr.port())).unwrap();
    let session = retina::client::Session::describe(url, SessionOptions::default())
        .await
        .expect("RTSPS describe should succeed");
    assert_eq!(session.streams().len(), 1);
    assert_eq!(session.streams()[0].encoding_name(), "h264");
    server_task.await.unwrap();
}
