use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use qam::slack::SlackUploader;

#[tokio::test]
async fn test_resolve_channel_mock_and_cases() {
    let mock_server = MockServer::start().await;

    // Mock conversations.list
    Mock::given(method("GET"))
        .and(path("/conversations.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "channels": [
                { "id": "C111", "name": "general" }
            ],
            "response_metadata": { "next_cursor": "" }
        })))
        .mount(&mock_server)
        .await;

    // Mock users.list
    Mock::given(method("GET"))
        .and(path("/users.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "members": [
                {
                    "id": "U222",
                    "name": "alice",
                    "profile": { "display_name": "Alice Wonderland" }
                }
            ],
            "response_metadata": { "next_cursor": "" }
        })))
        .mount(&mock_server)
        .await;

    // Mock conversations.open
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "channel": { "id": "D333" }
        })))
        .mount(&mock_server)
        .await;

    let uploader = SlackUploader::new("test-token").with_api_base(mock_server.uri());

    let cases = [
        ("C111", "C111", false),
        ("#general", "C111", false),
        ("general", "C111", false),
        ("@alice", "D333", false),
        ("alice", "D333", false),
        ("U222", "D333", false),
        ("nonexistent", "", true),
    ];

    for (input, want, want_err) in cases {
        let res = uploader.resolve_channel(input).await;
        if want_err {
            assert!(res.is_err(), "expected error for {input:?}");
        } else {
            let got = res.unwrap_or_else(|e| panic!("unexpected error for {input:?}: {e}"));
            assert_eq!(got, want, "resolve_channel({input:?})");
        }
    }
}

#[tokio::test]
async fn test_conversations_list_pagination() {
    let mock_server = MockServer::start().await;

    // Page 1: cursor "page2"
    Mock::given(method("GET"))
        .and(path("/conversations.list"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "channels": [
                { "id": "C001", "name": "random" }
            ],
            "response_metadata": { "next_cursor": "page2" }
        })))
        .mount(&mock_server)
        .await;

    // Page 2: channel "target"
    Mock::given(method("GET"))
        .and(path("/conversations.list"))
        .and(query_param("cursor", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "channels": [
                { "id": "C999", "name": "target" }
            ],
            "response_metadata": { "next_cursor": "" }
        })))
        .mount(&mock_server)
        .await;

    let uploader = SlackUploader::new("test-token").with_api_base(mock_server.uri());
    let id = uploader.resolve_channel("target").await.unwrap();
    assert_eq!(id, "C999");
}

#[tokio::test]
async fn test_upload_file_3_step_success_and_status_codes() {
    let mock_server = MockServer::start().await;

    // Step 1: files.getUploadURLExternal
    Mock::given(method("GET"))
        .and(path("/files.getUploadURLExternal"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "upload_url": format!("{}/upload-endpoint", mock_server.uri()),
            "file_id": "F123456"
        })))
        .mount(&mock_server)
        .await;

    // Step 2: raw POST to upload_url (returns 201 Created or 204 No Content)
    Mock::given(method("POST"))
        .and(path("/upload-endpoint"))
        .and(header("content-type", "image/jpeg"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    // Step 3: files.completeUploadExternal
    Mock::given(method("POST"))
        .and(path("/files.completeUploadExternal"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "files": [
                { "id": "F123456", "name": "snap.jpg" }
            ]
        })))
        .mount(&mock_server)
        .await;

    let tmp = tempdir().unwrap();
    let snap_path = tmp.path().join("snap.jpg");
    tokio::fs::write(&snap_path, b"test-jpeg-bytes")
        .await
        .unwrap();

    let uploader = SlackUploader::new("test-token").with_api_base(mock_server.uri());
    let res = uploader
        .upload_file(&snap_path, "C111", "Initial comment")
        .await
        .unwrap();
    assert!(res.ok);
    assert_eq!(res.files.len(), 1);
    assert_eq!(res.files[0].id, "F123456");
}

#[tokio::test]
async fn test_upload_file_step2_500_failure() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/files.getUploadURLExternal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "upload_url": format!("{}/fail-endpoint", mock_server.uri()),
            "file_id": "F999"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/fail-endpoint"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal server error"))
        .mount(&mock_server)
        .await;

    let tmp = tempdir().unwrap();
    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"content").await.unwrap();

    let uploader = SlackUploader::new("tok").with_api_base(mock_server.uri());
    let err = uploader.upload_file(&file, "C1", "").await.unwrap_err();
    assert!(err.to_string().contains("upload failed with status 500"));
}

#[test]
fn test_slack_set_cli_with_and_without_config() {
    let tmp = tempdir().unwrap();

    // 1. slack set without --config (uses default XDG path)
    let mut cmd1 = Command::cargo_bin("qam").unwrap();
    cmd1.env("XDG_CONFIG_HOME", tmp.path());
    cmd1.args(["slack", "set", "--token", "tok123", "--channel", "#general"]);
    cmd1.assert()
        .success()
        .stdout(predicate::str::contains("Slack configuration updated in"));

    // Verify written config
    let cfg_path = tmp.path().join("qam").join("config.yaml");
    let cfg = qam::config::load_from_path(&cfg_path).unwrap();
    assert_eq!(cfg.slack.token, "tok123");
    assert_eq!(cfg.slack.default_channel, "#general");

    // 2. slack set with explicit --config
    let custom_cfg = tmp.path().join("custom").join("config.yaml");
    let mut cmd2 = Command::cargo_bin("qam").unwrap();
    cmd2.args([
        "--config",
        custom_cfg.to_str().unwrap(),
        "slack",
        "set",
        "--token",
        "tok_custom",
    ]);
    cmd2.assert()
        .success()
        .stdout(predicate::str::contains("Slack configuration updated in"));

    let reloaded = qam::config::load_from_path(&custom_cfg).unwrap();
    assert_eq!(reloaded.slack.token, "tok_custom");
}

#[test]
fn test_slack_test_missing_file_arg() {
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.args(["slack", "test"]);
    cmd.assert().failure().code(1);
}
