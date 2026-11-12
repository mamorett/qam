use std::time::Duration;

use qam::cli::flags::{RtspAuth, Transport};
use qam::errors::classify;
use qam::media::session::RtspSession;
use retina::codec::FrameFormat;

mod support;
use support::fake_rtsp::{AuthMode, FakeRtspConfig, FakeRtspServer, Fault};
use support::fixtures::generate_synthetic_h264;

#[tokio::test]
async fn test_rtsp_session_tcp_connect_and_stream() {
    let frames = generate_synthetic_h264(3, 320, 240);
    let server = FakeRtspServer::start(FakeRtspConfig {
        video_h264_frames: frames,
        ..Default::default()
    })
    .await;

    let mut session = RtspSession::connect(
        &server.url(),
        Transport::Tcp,
        RtspAuth::Auto,
        Duration::from_secs(5),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .expect("connect TCP");

    let mut received_frames = 0;
    while let Ok(Some(frame)) = session.next_frame().await {
        received_frames += 1;
        assert!(frame.is_video());
        if received_frames >= 2 {
            break;
        }
    }
    assert!(received_frames >= 2);

    session.teardown().await;
    assert!(server.teardown_received());
}

#[tokio::test]
async fn test_rtsp_session_udp_connect() {
    let server = FakeRtspServer::start(FakeRtspConfig::default()).await;

    let session = RtspSession::connect(
        &server.url(),
        Transport::Udp,
        RtspAuth::Auto,
        Duration::from_secs(5),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .expect("connect UDP");

    session.teardown().await;
    assert!(server.teardown_received());
}

#[tokio::test]
async fn test_rtsp_session_basic_and_digest_auth() {
    // 1. Basic auth
    let server_basic = FakeRtspServer::start(FakeRtspConfig {
        auth_mode: AuthMode::Basic,
        username: "myuser".into(),
        password: "mypassword".into(),
        ..Default::default()
    })
    .await;

    let authed_url = format!(
        "rtsp://myuser:mypassword@127.0.0.1:{}/live",
        server_basic
            .url()
            .split(':')
            .nth(2)
            .unwrap()
            .split('/')
            .next()
            .unwrap()
    );

    let session_basic = RtspSession::connect(
        &authed_url,
        Transport::Tcp,
        RtspAuth::Basic,
        Duration::from_secs(5),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .expect("connect with Basic auth");
    session_basic.teardown().await;

    // 2. Digest auth
    let server_digest = FakeRtspServer::start(FakeRtspConfig {
        auth_mode: AuthMode::Digest,
        username: "myuser".into(),
        password: "mypassword".into(),
        ..Default::default()
    })
    .await;

    let digest_url = format!(
        "rtsp://myuser:mypassword@127.0.0.1:{}/live",
        server_digest
            .url()
            .split(':')
            .nth(2)
            .unwrap()
            .split('/')
            .next()
            .unwrap()
    );

    let session_digest = RtspSession::connect(
        &digest_url,
        Transport::Tcp,
        RtspAuth::Digest,
        Duration::from_secs(5),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .expect("connect with Digest auth");
    session_digest.teardown().await;
}

#[tokio::test]
async fn test_rtsp_session_auth_failure_classified() {
    let server = FakeRtspServer::start(FakeRtspConfig {
        auth_mode: AuthMode::Always401,
        ..Default::default()
    })
    .await;

    let err = RtspSession::connect(
        &server.url(),
        Transport::Tcp,
        RtspAuth::Auto,
        Duration::from_secs(5),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .unwrap_err();

    assert_eq!(classify(&err), "auth");
}

#[tokio::test]
async fn test_rtsp_session_network_refused_classified() {
    // Port 19998 has nothing listening
    let err = RtspSession::connect(
        "rtsp://127.0.0.1:19998/live",
        Transport::Tcp,
        RtspAuth::Auto,
        Duration::from_secs(2),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .unwrap_err();

    assert_eq!(classify(&err), "network-refused");
}

#[tokio::test]
async fn test_rtsp_session_timeout_classified() {
    let server = FakeRtspServer::start(FakeRtspConfig {
        fault: Fault::Stall,
        ..Default::default()
    })
    .await;

    let err = RtspSession::connect(
        &server.url(),
        Transport::Tcp,
        RtspAuth::Auto,
        Duration::from_millis(50),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .unwrap_err();

    assert_eq!(classify(&err), "network-timeout");
}

#[tokio::test]
async fn test_rtsp_session_not_found_classified() {
    let server = FakeRtspServer::start(FakeRtspConfig {
        expected_path: "/correct_path".into(),
        ..Default::default()
    })
    .await;

    let err = RtspSession::connect(
        &server.url_with_path("/nonexistent"),
        Transport::Tcp,
        RtspAuth::Auto,
        Duration::from_secs(5),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .unwrap_err();

    assert_eq!(classify(&err), "not-found");
}

#[tokio::test]
async fn test_rtsps_tls_self_signed_cert_succeeds() {
    let server = FakeRtspServer::start(FakeRtspConfig {
        use_tls: true,
        ..Default::default()
    })
    .await;

    let session = RtspSession::connect(
        &server.url(),
        Transport::Tcp,
        RtspAuth::Auto,
        Duration::from_secs(5),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .expect("rtsps with self-signed cert should succeed");

    session.teardown().await;
}

#[tokio::test]
async fn test_rtsp_credentials_percent_encoded_special_chars() {
    let server = FakeRtspServer::start(FakeRtspConfig {
        auth_mode: AuthMode::Basic,
        username: "user@domain".into(),
        password: "p@ss:w/rd?".into(),
        ..Default::default()
    })
    .await;

    // Build URL with percent-encoded user & password
    let encoded_url = format!(
        "rtsp://user%40domain:p%40ss%3Aw%2Frd%3F@127.0.0.1:{}/live",
        server
            .url()
            .split(':')
            .nth(2)
            .unwrap()
            .split('/')
            .next()
            .unwrap()
    );

    let session = RtspSession::connect(
        &encoded_url,
        Transport::Tcp,
        RtspAuth::Basic,
        Duration::from_secs(5),
        false,
        FrameFormat::SIMPLE,
    )
    .await
    .expect("connect with percent-encoded credentials");

    session.teardown().await;
}
