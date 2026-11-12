use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

mod support;
use support::fake_rtsp::{FakeRtspConfig, FakeRtspServer};

fn generate_scene_change_h264(width: usize, height: usize) -> Vec<Vec<u8>> {
    use openh264::encoder::Encoder as H264Encoder;
    use openh264::formats::YUVBuffer;

    let mut enc = H264Encoder::new().expect("failed to create H264Encoder");
    let y_len = width * height;
    let uv_len = (width / 2) * (height / 2);
    let total_len = y_len + 2 * uv_len;

    // Frame 0: solid black (Y=0)
    let mut f0 = vec![128u8; total_len];
    f0[..y_len].fill(0);
    let yuv0 = YUVBuffer::from_vec(f0, width, height);
    let b0 = enc.encode(&yuv0).expect("encode frame 0").to_vec();

    // Frame 1: solid white (Y=255)
    let mut f1 = vec![128u8; total_len];
    f1[..y_len].fill(255);
    let yuv1 = YUVBuffer::from_vec(f1, width, height);
    let b1 = enc.encode(&yuv1).expect("encode frame 1").to_vec();

    vec![b0, b1]
}

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

#[test]
fn test_watch_validations() {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yaml");
    fs::write(
        &cfg_path,
        "cameras:\n  - name: testcam\n    host: 127.0.0.1\n",
    )
    .unwrap();
    let cfg_str = cfg_path.to_str().unwrap();

    // 1. Missing camera
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(cfg_str)
        .arg("watch")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--camera is required"));

    // 2. Missing action
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(cfg_str)
        .args(["watch", "testcam"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--action is required (e.g., \"say motion\" or \"touch /tmp/motion\")",
        ));

    // 3. Threshold <= 0
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(cfg_str)
        .args(["watch", "testcam", "--action", "echo", "--threshold", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--threshold must be between 0 and 1 (e.g., 0.2)",
        ));

    // 4. Threshold >= 1
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(cfg_str)
        .args(["watch", "testcam", "--action", "echo", "--threshold", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--threshold must be between 0 and 1 (e.g., 0.2)",
        ));

    // 5. Invalid rtsp-auth
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(cfg_str)
        .args([
            "watch",
            "testcam",
            "--action",
            "echo",
            "--rtsp-auth",
            "invalid",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "invalid --rtsp-auth (use auto|basic|digest)",
        ));

    // 6. Invalid rtsp-transport
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(cfg_str)
        .args([
            "watch",
            "testcam",
            "--action",
            "echo",
            "--rtsp-transport",
            "invalid",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "invalid --rtsp-transport (use tcp|udp)",
        ));

    // 7. Camera not found
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(cfg_str)
        .args(["watch", "unknown", "--action", "echo"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("camera \"unknown\" not found"));

    // 8. Both stream and path
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config")
        .arg(cfg_str)
        .args([
            "watch", "testcam", "--action", "echo", "--stream", "stream1", "--path", "/path1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "use --path for custom RTSP token URLs; omit --stream",
        ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_watch_motion_trigger_action_and_text_log() {
    let frames = generate_scene_change_h264(320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let marker_dir = tempdir().unwrap();
    let marker_file = marker_dir.path().join("marker.txt");
    let marker_str = marker_file.to_str().unwrap();

    let action = format!("echo motion > {marker_str}");

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config").arg(&cfg_path).args([
        "watch",
        "testcam",
        "--threshold",
        "0.2",
        "--cooldown",
        "5s",
        "--duration",
        "1s",
        "--action",
        &action,
    ]);

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains("event=motion camera=testcam score=1.000 action=\"echo motion >"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("time="), "stdout was: {stdout}");

    // Give child process a moment to flush file
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(marker_file.exists());
    let content = fs::read_to_string(&marker_file).unwrap();
    assert_eq!(content.trim(), "motion");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_watch_json_logging() {
    let frames = generate_scene_change_h264(320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config").arg(&cfg_path).args([
        "watch",
        "testcam",
        "--threshold",
        "0.2",
        "--duration",
        "1s",
        "--json",
        "--action",
        "echo hi",
    ]);

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains(r#"{"event":"motion","camera":"testcam","score":1.000,"time":""#),
        "stdout was: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_watch_action_env_vars() {
    let frames = generate_scene_change_h264(320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("env_vars.txt");
    let out_str = out_file.to_str().unwrap();

    let action = format!("echo $CAMSNAP_CAMERA,$CAMSNAP_SCORE,$QAM_CAMERA,$QAM_SCORE > {out_str}");

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config").arg(&cfg_path).args([
        "watch",
        "testcam",
        "--threshold",
        "0.2",
        "--duration",
        "1s",
        "--action",
        &action,
    ]);

    cmd.assert().success();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(out_file.exists());
    let content = fs::read_to_string(&out_file).unwrap();
    assert_eq!(content.trim(), "testcam,1.000,testcam,1.000");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_watch_action_template() {
    let frames = generate_scene_change_h264(320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("template_out.txt");
    let out_str = out_file.to_str().unwrap();

    let tmpl = format!("echo {{camera}}_{{score}} > {out_str}");

    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--config").arg(&cfg_path).args([
        "watch",
        "testcam",
        "--threshold",
        "0.2",
        "--duration",
        "1s",
        "--action",
        "dummy",
        "--action-template",
        &tmpl,
    ]);

    cmd.assert().success();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(out_file.exists());
    let content = fs::read_to_string(&out_file).unwrap();
    assert_eq!(content.trim(), "testcam_1.000");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_watch_no_ffmpeg_in_path() {
    let frames = generate_scene_change_h264(320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let (_dir, cfg_path) = setup_test_camera(&server.url());
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("no_ffmpeg.txt");
    let out_str = out_file.to_str().unwrap();

    let action = format!("echo zero_ffmpeg > {out_str}");

    let mut cmd = Command::cargo_bin("qam").unwrap();
    // Prove zero ffmpeg dependency by wiping PATH
    cmd.env("PATH", "").arg("--config").arg(&cfg_path).args([
        "watch",
        "testcam",
        "--threshold",
        "0.2",
        "--duration",
        "1s",
        "--action",
        &action,
    ]);

    cmd.assert().success();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(out_file.exists());
    let content = fs::read_to_string(&out_file).unwrap();
    assert_eq!(content.trim(), "zero_ffmpeg");
}
