use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

mod support;
use support::fake_rtsp::{AuthMode, FakeRtspConfig, FakeRtspServer};
use support::fixtures::generate_synthetic_h264;

fn setup_test_camera(name: &str, host: &str, port: u16, path: &str) -> (tempfile::TempDir, String) {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yaml");

    let config_yaml = format!(
        "cameras:\n  - name: {name}\n    host: {host}\n    port: {port}\n    path: {path}\n"
    );
    fs::write(&cfg_path, config_yaml).unwrap();

    let cfg_str = cfg_path.to_str().unwrap().to_string();
    (dir, cfg_str)
}

#[test]
fn test_doctor_no_cameras() {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yaml");
    fs::write(&cfg_path, "cameras: []\n").unwrap();
    let cfg_str = cfg_path.to_str().unwrap();

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config").arg(cfg_str).arg("doctor");

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(stdout.contains("✔ built-in media engine"));
    assert!(stdout.contains("Config file:"));
    assert!(stdout.contains("No cameras saved. Add one with camsnap add ..."));
}

#[tokio::test]
async fn test_doctor_reachable_camera_without_probe() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let (_dir, cfg_path) = setup_test_camera("cam1", "127.0.0.1", port, "/live");

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config").arg(&cfg_path).arg("doctor");

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains(&format!("cam1 reachable at 127.0.0.1:{port}")),
        "stdout was: {stdout}"
    );
}

#[tokio::test]
async fn test_doctor_dial_failed() {
    // Bind and immediately drop listener to get an unused port
    let port = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };

    let (_dir, cfg_path) = setup_test_camera("cam_dead", "127.0.0.1", port, "/live");

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(&cfg_path)
        .args(["doctor", "--timeout", "200ms"]);

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains(&format!("cam_dead dial 127.0.0.1:{port} failed:")),
        "stdout was: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_doctor_probe_success() {
    let frames = generate_synthetic_h264(3, 320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let parsed = url::Url::parse(&server.url()).unwrap();
    let port = parsed.port().unwrap();

    let (_dir, cfg_path) = setup_test_camera("live_cam", "127.0.0.1", port, parsed.path());

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(&cfg_path)
        .args(["doctor", "--probe"]);

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains(&format!("live_cam reachable at 127.0.0.1:{port}")),
        "stdout was: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_doctor_probe_auth_failure_retries_and_classifies() {
    let frames = generate_synthetic_h264(2, 320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        auth_mode: AuthMode::Always401,
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let parsed = url::Url::parse(&server.url()).unwrap();
    let port = parsed.port().unwrap();

    // Config has wrong credentials
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yaml");
    let config_yaml = format!(
        "cameras:\n  - name: authcam\n    host: 127.0.0.1\n    port: {port}\n    user: admin\n    pass: wrongpassword\n"
    );
    fs::write(&cfg_path, config_yaml).unwrap();
    let cfg_str = cfg_path.to_str().unwrap();

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(cfg_str)
        .args(["doctor", "--probe", "--timeout", "200ms"]);

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains("authcam probe failed:"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("(auth)"), "stdout was: {stdout}");
}

#[tokio::test]
async fn test_doctor_invalid_rtsp_auth_reported_per_camera() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let (_dir, cfg_path) = setup_test_camera("cam_bad_auth", "127.0.0.1", port, "/live");

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(&cfg_path)
        .args(["doctor", "--probe", "--rtsp-auth", "bogus"]);

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains("cam_bad_auth probe failed: invalid --rtsp-auth (use auto|basic|digest)"),
        "stdout was: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_doctor_no_ffmpeg_in_path() {
    let frames = generate_synthetic_h264(3, 320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let parsed = url::Url::parse(&server.url()).unwrap();
    let port = parsed.port().unwrap();

    let (_dir, cfg_path) = setup_test_camera("live_cam", "127.0.0.1", port, parsed.path());

    let mut cmd = Command::cargo_bin("qam").unwrap();
    // Prove zero ffmpeg dependency by wiping PATH
    cmd.env("PATH", "")
        .arg("--config")
        .arg(&cfg_path)
        .args(["doctor", "--probe"]);

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(stdout.contains("✔ built-in media engine"));
    assert!(stdout.contains(&format!("live_cam reachable at 127.0.0.1:{port}")));
}
