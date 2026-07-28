//! CLI smoke tests (no printer / no real config dir required).

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

fn thermark() -> Command {
    Command::cargo_bin("thermark").expect("binary thermark")
}

/// Isolate from the developer's real config and THERMARK_ADDR.
fn thermark_with_config(path: &Path) -> Command {
    let mut c = thermark();
    c.env("THERMARK_CONFIG", path);
    c.env_remove("THERMARK_ADDR");
    c
}

fn temp_config_path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    (dir, path)
}

#[test]
fn help_exits_zero() {
    thermark()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("thermal label"));
}

#[test]
fn config_help_lists_subcommands() {
    thermark()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("clear"));
}

#[test]
fn tasks_prints_matrix() {
    thermark()
        .arg("tasks")
        .assert()
        .success()
        .stdout(predicate::str::contains("b1"))
        .stdout(predicate::str::contains("tested"));
}

#[test]
fn encode_rfid_probe() {
    thermark()
        .args(["encode", "1a", "01"])
        .assert()
        .success()
        .stdout(predicate::str::contains("55551a01011aaaaa"));
}

#[test]
fn bad_model_rejected() {
    thermark()
        .args(["info", "-a", "x", "--model", "not-a-model"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("possible values"));
}

#[test]
fn bad_task_rejected() {
    thermark()
        .args(["print", "-a", "x", "-i", "nope.png", "--task", "not-a-task"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("possible values"));
}

#[test]
fn fonts_runs() {
    thermark().arg("fonts").assert().success();
}

#[test]
fn doctor_host_only_runs() {
    let assert = thermark().arg("doctor").assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("thermark doctor"),
        "unexpected doctor output: {stdout}"
    );
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "status {:?}",
        output.status
    );
}

#[test]
fn config_set_show_clear() {
    let (_dir, path) = temp_config_path();

    thermark_with_config(&path)
        .args(["config", "set", "-a", "B1-TestPrinter", "--scan-secs", "7"])
        .assert()
        .success()
        .stdout(predicate::str::contains("B1-TestPrinter"));

    assert!(path.exists(), "config.json should be created");
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.trim_start().starts_with('{'), "{body}");
    assert!(body.contains("B1-TestPrinter"));
    assert!(body.contains("scan_secs") || body.contains("\"scan_secs\""));

    thermark_with_config(&path)
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("B1-TestPrinter"))
        .stdout(predicate::str::contains('7'));

    thermark_with_config(&path)
        .args(["config", "show", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("B1-TestPrinter"))
        .stdout(predicate::str::contains('{'));

    thermark_with_config(&path)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config.json"));

    thermark_with_config(&path)
        .args(["config", "clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));

    assert!(!path.exists());
}

#[test]
fn config_set_merges_model_and_connection() {
    let (_dir, path) = temp_config_path();

    thermark_with_config(&path)
        .args(["config", "set", "-a", "B1-A", "-c", "usb", "-m", "b21"])
        .assert()
        .success();

    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("B1-A"));
    assert!(body.contains("usb"));
    assert!(body.contains("b21"));

    thermark_with_config(&path)
        .args(["config", "set", "-a", "B1-B", "-c", "ble"])
        .assert()
        .success();

    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("B1-B"));
    assert!(body.contains("ble"));
    assert!(
        body.contains("b21"),
        "model should be preserved on merge: {body}"
    );
}

#[test]
fn config_show_empty() {
    let (_dir, path) = temp_config_path();
    thermark_with_config(&path)
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no config file yet"));
}

#[test]
fn info_without_addr_errors_helpfully() {
    let (_dir, path) = temp_config_path();
    thermark_with_config(&path)
        .arg("info")
        .assert()
        .failure()
        .stderr(predicate::str::contains("config set"));
}

#[test]
fn doctor_use_config_without_saved_addr_fails() {
    let (_dir, path) = temp_config_path();
    thermark_with_config(&path)
        .args(["doctor", "--use-config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("config set"));
}

#[test]
fn thermark_addr_env_used_when_no_flag() {
    let (_dir, path) = temp_config_path();
    let assert = thermark_with_config(&path)
        .env("THERMARK_ADDR", "B1-EnvOnlyFake")
        .args(["info", "--scan-secs", "1"])
        .assert()
        .failure();
    let err = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        !err.contains("no printer address"),
        "expected BLE/connect failure, got missing-addr: {err}"
    );
}
