use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

mod support;
use support::fake_rtsp::{FakeRtspConfig, FakeRtspServer};
use support::fixtures::{generate_g711_pcma, generate_synthetic_h264};

fn setup_test_camera(server_url: &str) -> (tempfile::TempDir, String) {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yaml");

    let parsed = url::Url::parse(server_url).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port().unwrap();
    let path = parsed.path();

    let config_yaml = format!(
        "cameras:\n  - name: testcam\n    host: {host}\n    port: {port}\n    path: {path}\n"
    );
    fs::write(&cfg_path, config_yaml).unwrap();

    let cfg_str = cfg_path.to_str().unwrap().to_string();
    (dir, cfg_str)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_record_clip_mp4_success() {
    let frames = generate_synthetic_h264(5, 320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("clip.mp4");
    let out_str = out_file.to_str().unwrap();

    let mut cmd = Command::cargo_bin("qam").unwrap();
    // Prove zero ffmpeg dependency by enforcing empty PATH
    cmd.env("PATH", "").arg("--config").arg(&cfg_path).args([
        "clip",
        "testcam",
        "--dur",
        "1s",
        "--timeout",
        "1s",
        "--out",
        out_str,
    ]);

    cmd.assert().success();

    assert!(out_file.exists());
    let bytes = fs::read(&out_file).unwrap();
    assert!(bytes.len() > 16);
    // MP4 begins with ftyp box at byte 4: b"ftyp"
    assert_eq!(&bytes[4..8], b"ftyp");

    // Verify it is a valid MP4 with mp4 crate reader
    let file = fs::File::open(&out_file).unwrap();
    let size = file.metadata().unwrap().len();
    let mp4_reader = mp4::Mp4Reader::read_header(file, size).expect("valid mp4 header");
    assert_eq!(mp4_reader.tracks().len(), 1);
    assert!(mp4_reader.size() > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_record_audio_wav_success() {
    let pcma_samples = generate_g711_pcma(1600); // 200 ms of 8000 Hz audio
    let server = FakeRtspServer::start(FakeRtspConfig {
        audio_pcma_bytes: pcma_samples,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("audio.wav");
    let out_str = out_file.to_str().unwrap();

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("PATH", "").arg("--config").arg(&cfg_path).args([
        "audio",
        "testcam",
        "--dur",
        "1s",
        "--timeout",
        "1s",
        "--out",
        out_str,
    ]);

    cmd.assert().success();

    assert!(out_file.exists());
    let bytes = fs::read(&out_file).unwrap();
    // RIFF header
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");

    let wav_reader = hound::WavReader::open(&out_file).expect("valid wav");
    assert_eq!(wav_reader.spec().channels, 1);
    assert_eq!(wav_reader.spec().sample_rate, 8000);
    assert_eq!(wav_reader.spec().bits_per_sample, 16);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_record_audio_alias() {
    let pcma_samples = generate_g711_pcma(1600);
    let server = FakeRtspServer::start(FakeRtspConfig {
        audio_pcma_bytes: pcma_samples,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("audio_alias.wav");
    let out_str = out_file.to_str().unwrap();

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("PATH", "").arg("--config").arg(&cfg_path).args([
        "record-audio",
        "testcam",
        "--dur",
        "1s",
        "--timeout",
        "1s",
        "--out",
        out_str,
    ]);

    cmd.assert().success();
    assert!(out_file.exists());
}

#[test]
fn test_clip_validations() {
    // Both --dur and --seconds rejected
    let mut cmd_both = Command::cargo_bin("qam").unwrap();
    cmd_both.args(["clip", "cam1", "--dur", "5s", "--seconds", "5"]);
    cmd_both
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "cannot specify both --dur and --seconds",
        ));

    // Both --audio-only and --no-audio rejected
    let mut cmd_audio_conflict = Command::cargo_bin("qam").unwrap();
    cmd_audio_conflict.args(["clip", "cam1", "--audio-only", "--no-audio"]);
    cmd_audio_conflict
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "cannot combine --audio-only and --no-audio",
        ));

    // --seconds must be > 0
    let mut cmd_sec_zero = Command::cargo_bin("qam").unwrap();
    cmd_sec_zero.args(["clip", "cam1", "--seconds", "0"]);
    cmd_sec_zero
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--seconds must be > 0"));

    // stdout output rejected for clip and audio
    let mut cmd_stdout_clip = Command::cargo_bin("qam").unwrap();
    cmd_stdout_clip.args(["clip", "cam1", "--out", "-"]);
    cmd_stdout_clip
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "stdout output not supported for clip",
        ));

    let mut cmd_stdout_audio = Command::cargo_bin("qam").unwrap();
    cmd_stdout_audio.args(["audio", "cam1", "--out", "-"]);
    cmd_stdout_audio
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "stdout output not supported for audio",
        ));
}
