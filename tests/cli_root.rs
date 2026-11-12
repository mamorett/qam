use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn test_version_flag() {
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::starts_with(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_version_short_flag() {
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("-v");
    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::starts_with(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_version_subcommand() {
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("version");
    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::starts_with(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_root_help() {
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd.arg("--help");
    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "qam – capture frames/clips and motion from RTSP/ONVIF cameras.",
        ))
        .stdout(predicate::str::contains("Common commands:"))
        .stdout(predicate::str::contains("Commands:"))
        .stdout(predicate::str::contains("Options:"))
        .stdout(predicate::str::contains("Examples:"));
}

#[test]
fn test_error_exit_code_1() {
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.arg("--unknown-flag-xyz");
    cmd.assert().failure().code(1);

    let mut cmd2 = Command::cargo_bin("qam").unwrap();
    cmd2.arg("unknown-cmd-xyz");
    cmd2.assert().failure().code(1);
}

#[test]
fn test_bool_equals_syntax() {
    let tmp = tempdir().unwrap();

    // Test --no-audio=true
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path());
    cmd.args([
        "add",
        "--name",
        "c1",
        "--host",
        "1.1.1.1",
        "--no-audio=true",
    ]);
    cmd.assert().success();

    // Test --no-audio=false
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path());
    cmd.args([
        "add",
        "--name",
        "c2",
        "--host",
        "1.1.1.2",
        "--no-audio=false",
    ]);
    cmd.assert().success();

    // Test --no-audio=1
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path());
    cmd.args(["add", "--name", "c3", "--host", "1.1.1.3", "--no-audio=1"]);
    cmd.assert().success();

    // Test --no-audio=0
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path());
    cmd.args(["add", "--name", "c4", "--host", "1.1.1.4", "--no-audio=0"]);
    cmd.assert().success();

    // Verify config saved values correctly
    let cfg_file = tmp.path().join("qam").join("config.yaml");
    let content = std::fs::read_to_string(cfg_file).unwrap();
    let cfg: qam::config::Config = serde_yaml::from_str(&content).unwrap();

    let c1 = cfg.find_camera("c1").unwrap();
    assert!(c1.no_audio, "c1 should have no_audio = true");

    let c2 = cfg.find_camera("c2").unwrap();
    assert!(!c2.no_audio, "c2 should have no_audio = false");

    let c3 = cfg.find_camera("c3").unwrap();
    assert!(c3.no_audio, "c3 should have no_audio = true");

    let c4 = cfg.find_camera("c4").unwrap();
    assert!(!c4.no_audio, "c4 should have no_audio = false");
}

#[test]
fn test_trailing_positionals_ignored() {
    let tmp = tempdir().unwrap();

    // add ignores extra trailing args
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path());
    cmd.args([
        "add", "--name", "c1", "--host", "1.1.1.1", "extra1", "extra2",
    ]);
    cmd.assert().success();

    // list ignores extra trailing args
    let mut cmd = Command::cargo_bin("qam").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path());
    cmd.args(["list", "extra3", "extra4"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("c1"));
}

#[test]
fn test_help_snapshots() {
    let commands = [
        vec!["--help"],
        vec!["add", "--help"],
        vec!["list", "--help"],
        vec!["snap", "--help"],
        vec!["clip", "--help"],
        vec!["audio", "--help"],
        vec!["discover", "--help"],
        vec!["watch", "--help"],
        vec!["doctor", "--help"],
        vec!["slack", "--help"],
        vec!["slack", "set", "--help"],
        vec!["slack", "test", "--help"],
        vec!["version", "--help"],
        vec!["completion", "--help"],
    ];

    for args in commands {
        let mut cmd = Command::cargo_bin("qam").unwrap();
        cmd.env("NO_COLOR", "1");
        cmd.args(&args);
        let assert = cmd.assert().success();
        let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        let out = out.replace("qam.exe", "qam");

        let snap_name = if args.len() == 1 {
            args[0].trim_start_matches('-').to_string()
        } else {
            args[..args.len() - 1].join("_")
        };
        insta::assert_snapshot!(format!("help_{}", snap_name), out);
    }
}
