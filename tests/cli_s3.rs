use tempfile::tempdir;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use qam::errors::classify;
use qam::s3::sigv4::sha256_hex;
use qam::s3::{Addressing, DEFAULT_KEY_TEMPLATE, S3Settings, S3Uploader};

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
