use std::fs;
use std::time::Duration;
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

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_clip_ctrl_c_finalizes_file_and_exits_130() {
    // 30 frames of synthetic video (~3 seconds)
    let frames = generate_synthetic_h264(30, 320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("clip_int.mp4");
    let out_str = out_file.to_str().unwrap();

    let bin_path = assert_cmd::cargo::cargo_bin("qam");
    let mut child = std::process::Command::new(bin_path)
        .arg("--config")
        .arg(&cfg_path)
        .args(["clip", "testcam", "--dur", "10s", "--out", out_str])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn qam");

    // Give it time to connect and write initial frames
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Send SIGINT to simulate Ctrl-C
    let kill_status = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(kill_status.success());

    let exit_status = child.wait().expect("wait for child");
    assert_eq!(exit_status.code(), Some(130));

    // File MUST be finalized and kept!
    assert!(out_file.exists());
    let size = fs::metadata(&out_file).unwrap().len();
    assert!(size > 0, "finalized MP4 file should not be empty");

    // Verify it's a valid MP4 with 'ftyp'
    let bytes = fs::read(&out_file).unwrap();
    assert!(bytes.len() >= 8);
    assert_eq!(&bytes[4..8], b"ftyp");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_audio_ctrl_c_finalizes_file_and_exits_130() {
    use support::fixtures::generate_g711_pcma;

    let audio = generate_g711_pcma(16000); // 2 seconds
    let server = FakeRtspServer::start(FakeRtspConfig {
        audio_pcma_bytes: audio,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("audio_int.wav");
    let out_str = out_file.to_str().unwrap();

    let bin_path = assert_cmd::cargo::cargo_bin("qam");
    let mut child = std::process::Command::new(bin_path)
        .arg("--config")
        .arg(&cfg_path)
        .args(["audio", "testcam", "--dur", "10s", "--out", out_str])
        .spawn()
        .expect("spawn qam");

    tokio::time::sleep(Duration::from_millis(400)).await;

    let kill_status = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(kill_status.success());

    let exit_status = child.wait().expect("wait for child");
    assert_eq!(exit_status.code(), Some(130));

    assert!(out_file.exists());
    let bytes = fs::read(&out_file).unwrap();
    assert!(bytes.len() >= 12);
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
}
