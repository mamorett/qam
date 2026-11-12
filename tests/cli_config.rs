use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn test_add_and_list() {
    let tmp = tempdir().unwrap();
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path());
    cmd.args([
        "add", "--name", "t1", "--host", "1.1.1.1", "--user", "u", "--pass", "p",
    ]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Added camera \"t1\""));

    let mut list_cmd = Command::cargo_bin("qam").unwrap();
    list_cmd.env("XDG_CONFIG_HOME", tmp.path());
    list_cmd.arg("list");
    list_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("t1"))
        .stdout(predicate::str::contains("host=1.1.1.1"))
        .stdout(predicate::str::contains("port=554"))
        .stdout(predicate::str::contains("proto=rtsp"))
        .stdout(predicate::str::contains("auth=u:***"));
}

#[test]
fn test_add_update_message() {
    let tmp = tempdir().unwrap();
    let mut cmd1 = Command::cargo_bin("qam").unwrap();
    cmd1.env("XDG_CONFIG_HOME", tmp.path());
    cmd1.args(["add", "--name", "cam1", "--host", "192.168.1.50"]);
    cmd1.assert()
        .success()
        .stdout(predicate::str::contains("Added camera \"cam1\""));

    let mut cmd2 = Command::cargo_bin("qam").unwrap();
    cmd2.env("XDG_CONFIG_HOME", tmp.path());
    cmd2.args(["add", "--name", "cam1", "--host", "192.168.1.51"]);
    cmd2.assert()
        .success()
        .stdout(predicate::str::contains("Updated camera \"cam1\""));

    let mut list_cmd = Command::cargo_bin("qam").unwrap();
    list_cmd.env("XDG_CONFIG_HOME", tmp.path());
    list_cmd.arg("list");
    list_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("host=192.168.1.51"));
}

#[test]
fn test_list_empty_warn() {
    let tmp = tempdir().unwrap();
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path());
    cmd.env("NO_COLOR", "1");
    cmd.arg("list");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("No cameras saved. Add one with: qam add --name cam1 --host 192.168.1.50 --user tapo --pass secret"));
}

#[test]
fn test_list_sort_and_masking() {
    let tmp = tempdir().unwrap();
    let mut add_b = Command::cargo_bin("qam").unwrap();
    add_b.env("XDG_CONFIG_HOME", tmp.path());
    add_b.args([
        "add",
        "--name",
        "zebra",
        "--host",
        "10.0.0.2",
        "--user",
        "admin",
        "--pass",
        "supersecret",
    ]);
    add_b.assert().success();

    let mut add_a = Command::cargo_bin("qam").unwrap();
    add_a.env("XDG_CONFIG_HOME", tmp.path());
    add_a.args([
        "add", "--name", "alpha", "--host", "10.0.0.1", "--user", "viewer", "--pass", "pass123",
    ]);
    add_a.assert().success();

    let mut list_cmd = Command::cargo_bin("qam").unwrap();
    list_cmd.env("XDG_CONFIG_HOME", tmp.path());
    list_cmd.arg("list");
    let assert = list_cmd.assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    // Verify alpha appears before zebra
    let pos_a = out.find("alpha").expect("alpha in output");
    let pos_z = out.find("zebra").expect("zebra in output");
    assert!(pos_a < pos_z, "alpha should precede zebra in sorted list");

    // Passwords must never appear
    assert!(!out.contains("supersecret"), "password must not appear");
    assert!(!out.contains("pass123"), "password must not appear");
    assert!(out.contains("auth=admin:***"));
    assert!(out.contains("auth=viewer:***"));
}

#[test]
fn test_add_validation_required_fields() {
    let tmp = tempdir().unwrap();
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path());
    cmd.arg("add");
    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("name and host are required"));

    let mut cmd2 = Command::cargo_bin("qam").unwrap();
    cmd2.env("XDG_CONFIG_HOME", tmp.path());
    cmd2.args(["add", "--name", "cam_only"]);
    cmd2.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("name and host are required"));
}
