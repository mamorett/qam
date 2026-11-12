use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

mod support;
use support::fake_rtsp::{FakeRtspConfig, FakeRtspServer};
use support::fixtures::generate_synthetic_h264;

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
async fn test_snap_jpeg_success_and_no_ffmpeg_in_path() {
    let frames = generate_synthetic_h264(3, 320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("snap.jpg");
    let out_str = out_file.to_str().unwrap();

    let mut cmd = Command::cargo_bin("qam").unwrap();
    // CRITICAL: Empty PATH ensures zero ffmpeg/ffprobe binary dependency!
    cmd.env("PATH", "")
        .arg("--config")
        .arg(&cfg_path)
        .args(["snap", "testcam", "--out", out_str]);

    cmd.assert().success();

    assert!(out_file.exists());
    let bytes = fs::read(&out_file).unwrap();
    assert!(!bytes.is_empty());
    // JPEG SOI marker
    assert_eq!(&bytes[0..2], &[0xFF, 0xD8]);
    // Read image using image crate to verify it's a completely valid JPEG
    let img = image::load_from_memory(&bytes).expect("valid jpeg");
    assert_eq!(img.width(), 320);
    assert_eq!(img.height(), 240);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_snap_to_stdout() {
    let frames = generate_synthetic_h264(2, 160, 120);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("PATH", "")
        .arg("--config")
        .arg(&cfg_path)
        .args(["snap", "--camera", "testcam", "--out", "-"]);

    let assert = cmd.assert().success();
    let stdout = assert.get_output().stdout.clone();
    assert!(!stdout.is_empty());
    assert_eq!(&stdout[0..2], &[0xFF, 0xD8]);

    let img = image::load_from_memory(&stdout).expect("valid jpeg from stdout");
    assert_eq!(img.width(), 160);
    assert_eq!(img.height(), 120);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_snap_png_output() {
    let frames = generate_synthetic_h264(2, 160, 120);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("snap.png");
    let out_str = out_file.to_str().unwrap();

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("PATH", "")
        .arg("--config")
        .arg(&cfg_path)
        .args(["snap", "testcam", "--out", out_str]);

    cmd.assert().success();

    assert!(out_file.exists());
    let bytes = fs::read(&out_file).unwrap();
    // PNG header: \x89PNG\r\n\x1a\n
    assert_eq!(&bytes[0..8], b"\x89PNG\r\n\x1a\n");

    let img = image::load_from_memory(&bytes).expect("valid png");
    assert_eq!(img.width(), 160);
    assert_eq!(img.height(), 120);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_snap_overwrite_refusal() {
    let frames = generate_synthetic_h264(2, 160, 120);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("snap.jpg");
    let out_str = out_file.to_str().unwrap();

    // Pre-create file
    fs::write(&out_file, b"existing content").unwrap();

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("PATH", "").arg("--config").arg(&cfg_path).args([
        "snap",
        "testcam",
        "--out",
        out_str,
        "--overwrite=false",
    ]);

    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "output file exists: {out_str}"
        )));

    // Verify existing content was NOT overwritten
    assert_eq!(fs::read(&out_file).unwrap(), b"existing content");

    // Now test with default overwrite=true
    let mut cmd_ok = Command::cargo_bin("qam").unwrap();
    cmd_ok
        .env("PATH", "")
        .arg("--config")
        .arg(&cfg_path)
        .args(["snap", "testcam", "--out", out_str]);

    cmd_ok.assert().success();
    assert_ne!(fs::read(&out_file).unwrap(), b"existing content");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_snap_quality_difference() {
    let frames = generate_synthetic_h264(2, 320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let q1_file = out_dir.path().join("q1.jpg");
    let q31_file = out_dir.path().join("q31.jpg");

    // Quality 1 (best quality, largest file)
    let mut cmd1 = Command::cargo_bin("qam").unwrap();
    cmd1.env("PATH", "").arg("--config").arg(&cfg_path).args([
        "snap",
        "testcam",
        "--out",
        q1_file.to_str().unwrap(),
        "--quality",
        "1",
    ]);
    cmd1.assert().success();

    // Quality 31 (worst quality, smallest file)
    let mut cmd31 = Command::cargo_bin("qam").unwrap();
    cmd31.env("PATH", "").arg("--config").arg(&cfg_path).args([
        "snap",
        "testcam",
        "--out",
        q31_file.to_str().unwrap(),
        "--quality",
        "31",
    ]);
    cmd31.assert().success();

    let size_q1 = fs::metadata(&q1_file).unwrap().len();
    let size_q31 = fs::metadata(&q31_file).unwrap().len();

    assert!(
        size_q1 > size_q31,
        "quality 1 size ({size_q1}) should be significantly larger than quality 31 size ({size_q31})"
    );
}

#[test]
fn test_snap_cli_validations() {
    let mut cmd_no_cam = Command::cargo_bin("qam").unwrap();
    cmd_no_cam.arg("snap");
    cmd_no_cam
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--camera is required"));

    let mut cmd_conflict = Command::cargo_bin("qam").unwrap();
    cmd_conflict.args(["snap", "cam1", "--stream", "s1", "--path", "/p1"]);
    cmd_conflict
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "use --path for custom RTSP token URLs; omit --stream",
        ));

    let tmp = tempdir().unwrap();
    let cfg = tmp.path().join("config.yaml");
    fs::write(&cfg, "cameras: {}\n").unwrap();

    let mut cmd_not_found = Command::cargo_bin("qam").unwrap();
    cmd_not_found
        .arg("--config")
        .arg(cfg.to_str().unwrap())
        .args(["snap", "nonexistent"]);
    cmd_not_found
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("camera \"nonexistent\" not found"));
}
