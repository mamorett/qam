use assert_cmd::Command;
use predicates::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::tempdir;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use qam::cli::deliver::{DeliveryRequest, SlackFlags, deliver};
use qam::config::{self, Config};
use qam::errors::classify;
use qam::s3::sigv4::sha256_hex;
use qam::s3::{Addressing, DEFAULT_KEY_TEMPLATE, S3Flags, S3Settings, S3Uploader};

fn test_settings(endpoint: &str) -> S3Settings {
    S3Settings {
        endpoint: Url::parse(endpoint).unwrap(),
        is_custom_endpoint: true,
        region: "us-east-1".into(),
        bucket: "test-bucket".into(),
        prefix: String::new(),
        key_template: DEFAULT_KEY_TEMPLATE.into(),
        access_key_id: "AKIDTEST".into(),
        secret_access_key: "SUPERSECRETTESTKEY".into(),
        session_token: None,
        addressing: Addressing::Path,
        public_url: Some("https://cdn.example.com".into()),
        storage_class: None,
    }
}

#[tokio::test]
async fn test_put_object_success_and_headers() {
    let mock_server = MockServer::start().await;
    let payload = b"fake-jpeg-bytes-12345";
    let expected_sha = sha256_hex(payload);

    Mock::given(method("PUT"))
        .and(path("/test-bucket/cam/snap.jpg"))
        .and(header("content-type", "image/jpeg"))
        .and(header("x-amz-content-sha256", expected_sha.as_str()))
        .respond_with(ResponseTemplate::new(200).insert_header("ETag", "\"etag-123\""))
        .expect(1)
        .mount(&mock_server)
        .await;

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("snap.jpg");
    std::fs::write(&file_path, payload).unwrap();

    let uploader = S3Uploader::new(test_settings(&mock_server.uri()));
    let res = uploader
        .put_object(&file_path, "cam/snap.jpg")
        .await
        .unwrap();

    assert_eq!(res.bucket, "test-bucket");
    assert_eq!(res.key, "cam/snap.jpg");
    assert_eq!(res.uri, "s3://test-bucket/cam/snap.jpg");
    assert_eq!(
        res.url.as_deref(),
        Some("https://cdn.example.com/cam/snap.jpg")
    );
    assert_eq!(res.etag.as_deref(), Some("\"etag-123\""));

    let reqs = mock_server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let req = &reqs[0];
    assert_eq!(req.body, payload);

    let auth = req.headers.get("authorization").unwrap().to_str().unwrap();
    assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDTEST/"));
    assert!(auth.contains("/us-east-1/s3/aws4_request"));
    assert!(auth.contains("SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date"));

    assert!(req.headers.get("transfer-encoding").is_none());
    for (k, _) in req.headers.iter() {
        assert!(
            !k.as_str().starts_with("x-amz-checksum-"),
            "unexpected checksum header: {k}"
        );
    }
}

#[tokio::test]
async fn test_put_object_403_signature_does_not_match() {
    let mock_server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/test-bucket/cam/snap.jpg"))
        .respond_with(ResponseTemplate::new(403).set_body_string(
            "<Error><Code>SignatureDoesNotMatch</Code><Message>The request signature we calculated does not match.</Message></Error>",
        ))
        .expect(1)
        .mount(&mock_server)
        .await;

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("snap.jpg");
    std::fs::write(&file_path, b"x").unwrap();

    let uploader = S3Uploader::new(test_settings(&mock_server.uri()));
    let err = uploader
        .put_object(&file_path, "cam/snap.jpg")
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("SignatureDoesNotMatch"), "msg: {msg}");
    assert!(msg.contains("auth"), "msg: {msg}");
    assert!(
        !msg.contains("SUPERSECRETTESTKEY"),
        "secret leaked in error!"
    );
    assert_eq!(classify(&err), "auth");
}

#[tokio::test]
async fn test_put_object_404_no_such_bucket() {
    let mock_server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/test-bucket/cam/snap.jpg"))
        .respond_with(ResponseTemplate::new(404).set_body_string(
            "<Error><Code>NoSuchBucket</Code><Message>The specified bucket does not exist</Message></Error>",
        ))
        .expect(1)
        .mount(&mock_server)
        .await;

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("snap.jpg");
    std::fs::write(&file_path, b"x").unwrap();

    let uploader = S3Uploader::new(test_settings(&mock_server.uri()));
    let err = uploader
        .put_object(&file_path, "cam/snap.jpg")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
    assert_eq!(classify(&err), "not-found");
}

#[tokio::test]
async fn test_put_object_301_not_followed_and_reports_region() {
    let mock_server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/test-bucket/cam/snap.jpg"))
        .respond_with(
            ResponseTemplate::new(301)
                .insert_header("Location", "https://other.example.com/snap.jpg")
                .insert_header("x-amz-bucket-region", "eu-west-1")
                .set_body_string("<Error><Code>PermanentRedirect</Code><Message>Use another endpoint</Message></Error>"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("snap.jpg");
    std::fs::write(&file_path, b"x").unwrap();

    let uploader = S3Uploader::new(test_settings(&mock_server.uri()));
    let err = uploader
        .put_object(&file_path, "cam/snap.jpg")
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("eu-west-1"), "msg: {msg}");

    let reqs = mock_server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
}

#[tokio::test]
async fn test_put_object_503_retries_once() {
    let mock_server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/test-bucket/cam/snap.jpg"))
        .respond_with(ResponseTemplate::new(503).set_body_string("SlowDown"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/test-bucket/cam/snap.jpg"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("snap.jpg");
    std::fs::write(&file_path, b"x").unwrap();

    let uploader = S3Uploader::new(test_settings(&mock_server.uri()));
    uploader
        .put_object(&file_path, "cam/snap.jpg")
        .await
        .unwrap();

    let reqs = mock_server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 2);
}

#[tokio::test]
async fn test_put_object_with_session_token() {
    let mock_server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/test-bucket/cam/snap.jpg"))
        .and(header("x-amz-security-token", "tok-abc"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("snap.jpg");
    std::fs::write(&file_path, b"x").unwrap();

    let mut settings = test_settings(&mock_server.uri());
    settings.session_token = Some("tok-abc".into());
    let uploader = S3Uploader::new(settings);
    uploader
        .put_object(&file_path, "cam/snap.jpg")
        .await
        .unwrap();

    let reqs = mock_server.received_requests().await.unwrap();
    let auth = reqs[0]
        .headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(auth.contains("x-amz-security-token"));
}

#[tokio::test]
#[ignore]
async fn e2e_real_s3() {
    if std::env::var("QAM_S3_E2E").ok().as_deref() != Some("1") {
        return;
    }
    let cfg = qam::config::Config::default();
    let flags = qam::s3::S3Flags {
        enabled: Some(true),
        ..Default::default()
    };
    let settings = qam::s3::resolve_s3_settings(&flags, &cfg, &|k| std::env::var(k).ok()).unwrap();
    let uploader = S3Uploader::new(settings);

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("e2e.txt");
    std::fs::write(&file_path, b"qam-e2e-test").unwrap();

    let key = "qam-e2e/test.txt";
    let res = uploader.put_object(&file_path, key).await.unwrap();
    assert_eq!(res.key, key);
    uploader.head_object(key).await.unwrap();
    uploader.delete_object(key).await.unwrap();
}

fn qam_cmd() -> Command {
    let mut cmd = Command::cargo_bin("qam").unwrap();
    for var in [
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_REGION",
        "AWS_DEFAULT_REGION",
        "AWS_ENDPOINT_URL",
        "AWS_ENDPOINT_URL_S3",
        "AWS_PROFILE",
        "QAM_S3_ENDPOINT",
        "QAM_S3_REGION",
        "QAM_S3_BUCKET",
        "QAM_S3_PREFIX",
        "QAM_S3_ACCESS_KEY_ID",
        "QAM_S3_SECRET_ACCESS_KEY",
        "QAM_S3_SESSION_TOKEN",
        "QAM_S3_ADDRESSING",
        "QAM_S3_PUBLIC_URL",
        "SLACK_TOKEN",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

#[test]
fn test_s3_set_cli_roundtrip_and_partial_update() {
    let tmp = tempdir().unwrap();
    let cfg_path = tmp.path().join("config.yaml");

    qam_cmd()
        .args([
            "--config",
            cfg_path.to_str().unwrap(),
            "s3",
            "set",
            "--endpoint",
            "http://127.0.0.1:9000/",
            "--region",
            "auto",
            "--bucket",
            "cams",
            "--prefix",
            "home/",
            "--key-template",
            "{prefix}{camera}.{ext}",
            "--access-key-id",
            "AK1",
            "--secret-access-key",
            "SK1",
            "--session-token",
            "ST1",
            "--addressing",
            "path",
            "--public-url",
            "https://cdn.example.com/",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("S3 configuration updated in"))
        .stdout(predicate::str::contains("--s3"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cfg_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let cfg = config::load_from_path(&cfg_path).unwrap();
    assert_eq!(cfg.s3.endpoint, "http://127.0.0.1:9000");
    assert_eq!(cfg.s3.region, "auto");
    assert_eq!(cfg.s3.bucket, "cams");
    assert_eq!(cfg.s3.prefix, "home/");
    assert_eq!(cfg.s3.key_template, "{prefix}{camera}.{ext}");
    assert_eq!(cfg.s3.access_key_id, "AK1");
    assert_eq!(cfg.s3.secret_access_key, "SK1");
    assert_eq!(cfg.s3.session_token, "ST1");
    assert_eq!(cfg.s3.addressing, "path");
    assert_eq!(cfg.s3.public_url, "https://cdn.example.com");

    // Partial update: only --bucket changes, other fields preserved
    qam_cmd()
        .args([
            "--config",
            cfg_path.to_str().unwrap(),
            "s3",
            "set",
            "--bucket",
            "cams-v2",
        ])
        .assert()
        .success();

    let cfg2 = config::load_from_path(&cfg_path).unwrap();
    assert_eq!(cfg2.s3.bucket, "cams-v2");
    assert_eq!(cfg2.s3.endpoint, "http://127.0.0.1:9000");
    assert_eq!(cfg2.s3.access_key_id, "AK1");
    assert_eq!(cfg2.s3.secret_access_key, "SK1");
}

#[tokio::test]
async fn test_s3_test_cli_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/test-bucket/test-key.jpg"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("HEAD"))
        .and(path("/test-bucket/test-key.jpg"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let tmp = tempdir().unwrap();
    let cfg_path = tmp.path().join("config.yaml");
    let mut cfg = Config::default();
    cfg.s3.endpoint = mock_server.uri();
    cfg.s3.bucket = "test-bucket".into();
    config::save_to_path(&cfg_path, &cfg).unwrap();

    let file_path = tmp.path().join("shot.jpg");
    std::fs::write(&file_path, b"hello").unwrap();

    let output = tokio::task::spawn_blocking(move || {
        qam_cmd()
            .env("QAM_S3_ACCESS_KEY_ID", "AKTEST")
            .env("QAM_S3_SECRET_ACCESS_KEY", "SKTEST")
            .args([
                "--config",
                cfg_path.to_str().unwrap(),
                "s3",
                "test",
                file_path.to_str().unwrap(),
                "--key",
                "test-key.jpg",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone()
    })
    .await
    .unwrap();

    let stdout = String::from_utf8(output).unwrap();
    assert!(stdout.contains("Success! s3://test-bucket/test-key.jpg"));
}

#[test]
fn test_s3_test_missing_file_arg() {
    qam_cmd().args(["s3", "test"]).assert().failure();
}

async fn mount_slack_mocks(server: &MockServer, seq: &'static AtomicUsize) {
    struct SeqResponder(&'static AtomicUsize, serde_json::Value);
    impl Respond for SeqResponder {
        fn respond(&self, _req: &Request) -> ResponseTemplate {
            self.0.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(self.1.clone())
        }
    }

    let upload_url = format!("{}/upload_bin", server.uri());
    Mock::given(method("GET"))
        .and(path("/files.getUploadURLExternal"))
        .respond_with(SeqResponder(
            seq,
            serde_json::json!({ "ok": true, "upload_url": upload_url, "file_id": "F1" }),
        ))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/upload_bin"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/files.completeUploadExternal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "files": [{ "id": "F1", "name": "shot.jpg" }]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn test_deliver_opt_in_and_destination_combinations() {
    static SLACK_SEQ: AtomicUsize = AtomicUsize::new(0);
    let slack_mock = MockServer::start().await;
    let s3_mock = MockServer::start().await;

    mount_slack_mocks(&slack_mock, &SLACK_SEQ).await;

    Mock::given(method("PUT"))
        .and(path("/my-bucket/shot.jpg"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&s3_mock)
        .await;

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("shot.jpg");
    std::fs::write(&file_path, b"jpg").unwrap();

    let mut cfg = Config::default();
    cfg.slack.token = "xoxb-test".into();
    cfg.slack.default_channel = "C12345678".into();
    cfg.s3.endpoint = s3_mock.uri();
    cfg.s3.bucket = "my-bucket".into();
    cfg.s3.key_template = "shot.jpg".into();
    cfg.s3.access_key_id = "AK".into();
    cfg.s3.secret_access_key = "SK".into();

    // 1. Neither flag -> zero requests on both mocks, temp file kept
    let out = deliver(DeliveryRequest {
        out_path: file_path.to_str().unwrap(),
        is_temp_file: true,
        camera: "kitchen",
        cfg: &cfg,
        slack: SlackFlags::default(),
        s3: S3Flags::default(),
        slack_api_base: Some(&slack_mock.uri()),
    })
    .await
    .unwrap();
    assert!(!out.slack_uploaded);
    assert!(out.s3.is_none());
    assert!(file_path.exists());
    assert!(slack_mock.received_requests().await.unwrap().is_empty());
    assert!(s3_mock.received_requests().await.unwrap().is_empty());

    // 2. Slack only
    let out = deliver(DeliveryRequest {
        out_path: file_path.to_str().unwrap(),
        is_temp_file: false,
        camera: "kitchen",
        cfg: &cfg,
        slack: SlackFlags {
            enabled: Some(true),
            ..Default::default()
        },
        s3: S3Flags::default(),
        slack_api_base: Some(&slack_mock.uri()),
    })
    .await
    .unwrap();
    assert!(out.slack_uploaded);
    assert!(out.s3.is_none());
    assert!(s3_mock.received_requests().await.unwrap().is_empty());
    assert!(!slack_mock.received_requests().await.unwrap().is_empty());

    // 3. S3 only
    let prev_slack_count = slack_mock.received_requests().await.unwrap().len();
    let out = deliver(DeliveryRequest {
        out_path: file_path.to_str().unwrap(),
        is_temp_file: false,
        camera: "kitchen",
        cfg: &cfg,
        slack: SlackFlags::default(),
        s3: S3Flags {
            enabled: Some(true),
            ..Default::default()
        },
        slack_api_base: Some(&slack_mock.uri()),
    })
    .await
    .unwrap();
    assert!(!out.slack_uploaded);
    assert!(out.s3.is_some());
    assert_eq!(s3_mock.received_requests().await.unwrap().len(), 1);
    assert_eq!(
        slack_mock.received_requests().await.unwrap().len(),
        prev_slack_count
    );

    // 4. Both
    let out = deliver(DeliveryRequest {
        out_path: file_path.to_str().unwrap(),
        is_temp_file: false,
        camera: "kitchen",
        cfg: &cfg,
        slack: SlackFlags {
            enabled: Some(true),
            ..Default::default()
        },
        s3: S3Flags {
            enabled: Some(true),
            ..Default::default()
        },
        slack_api_base: Some(&slack_mock.uri()),
    })
    .await
    .unwrap();
    assert!(out.slack_uploaded);
    assert!(out.s3.is_some());
    assert_eq!(s3_mock.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_deliver_s3_failure_still_attempts_slack_and_keeps_temp_file() {
    static SLACK_SEQ: AtomicUsize = AtomicUsize::new(0);
    let slack_mock = MockServer::start().await;
    let s3_mock = MockServer::start().await;

    mount_slack_mocks(&slack_mock, &SLACK_SEQ).await;

    Mock::given(method("PUT"))
        .and(path("/my-bucket/shot.jpg"))
        .respond_with(
            ResponseTemplate::new(403).set_body_string(
                "<Error><Code>AccessDenied</Code><Message>Denied</Message></Error>",
            ),
        )
        .mount(&s3_mock)
        .await;

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("shot.jpg");
    std::fs::write(&file_path, b"jpg").unwrap();

    let mut cfg = Config::default();
    cfg.slack.token = "xoxb-test".into();
    cfg.slack.default_channel = "C12345678".into();
    cfg.s3.endpoint = s3_mock.uri();
    cfg.s3.bucket = "my-bucket".into();
    cfg.s3.key_template = "shot.jpg".into();
    cfg.s3.access_key_id = "AK".into();
    cfg.s3.secret_access_key = "SK".into();

    let err = deliver(DeliveryRequest {
        out_path: file_path.to_str().unwrap(),
        is_temp_file: true,
        camera: "kitchen",
        cfg: &cfg,
        slack: SlackFlags {
            enabled: Some(true),
            ..Default::default()
        },
        s3: S3Flags {
            enabled: Some(true),
            ..Default::default()
        },
        slack_api_base: Some(&slack_mock.uri()),
    })
    .await
    .unwrap_err();

    assert!(err.to_string().contains("AccessDenied"));
    assert!(file_path.exists(), "temp file must be kept on failure");
    assert!(
        !slack_mock.received_requests().await.unwrap().is_empty(),
        "Slack upload must still be attempted when S3 fails"
    );
}

#[test]
fn test_cli_flag_validation_fails_fast() {
    let tmp = tempdir().unwrap();
    let cfg_path = tmp.path().join("config.yaml");
    config::save_to_path(&cfg_path, &Config::default()).unwrap();

    // --s3-key without --s3
    qam_cmd()
        .args([
            "--config",
            cfg_path.to_str().unwrap(),
            "snap",
            "cam",
            "--s3-key",
            "x",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("without --s3"));

    // --s3=false with --s3-bucket
    qam_cmd()
        .args([
            "--config",
            cfg_path.to_str().unwrap(),
            "snap",
            "cam",
            "--s3=false",
            "--s3-bucket",
            "b",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("conflicting flags"));

    // --slack-message without --slack
    qam_cmd()
        .args([
            "--config",
            cfg_path.to_str().unwrap(),
            "snap",
            "cam",
            "--slack-message",
            "hi",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("without --slack"));

    // --slack=false with --slack-channel
    qam_cmd()
        .args([
            "--config",
            cfg_path.to_str().unwrap(),
            "snap",
            "cam",
            "--slack=false",
            "--slack-channel",
            "#x",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("conflicting flags"));
}
