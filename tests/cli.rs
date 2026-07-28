//! CLI smoke tests (no printer required).

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;

fn thermark() -> Command {
    Command::cargo_bin("thermark").expect("binary thermark")
}

fn thermark_with_config(path: &std::path::Path) -> Command {
    let mut c = thermark();
    c.env("THERMARK_CONFIG", path);
    // Isolate from developer machine env
    c.env_remove("THERMARK_ADDR");
    c
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
    // 55 55 1a 01 01 1a aa aa
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
    // No -a: host checks only (Bluetooth may pass or fail; process should exit 0 or 1 cleanly).
    let assert = thermark().arg("doctor").assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("thermark doctor") || stdout.contains("doctor"),
        "unexpected doctor output: {stdout}"
    );
    // Exit 0 (ok/warn) or 1 (fail e.g. no BT) — not a crash.
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "status {:?}",
        output.status
    );
}

#[test]
fn config_set_show_clear() {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("config.toml");

    thermark_with_config(&path)
        .args(["config", "set", "-a", "B1-TestPrinter", "--scan-secs", "7"])
        .assert()
        .success()
        .stdout(predicate::str::contains("B1-TestPrinter"));

    assert!(path.exists());

    thermark_with_config(&path)
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("B1-TestPrinter"))
        .stdout(predicate::str::contains("7"));

    thermark_with_config(&path)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config.toml"));

    thermark_with_config(&path)
        .args(["config", "clear"])
        .assert()
        .success();

    assert!(!path.exists());
}

#[test]
fn info_without_addr_errors_helpfully() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.toml");
    // file does not exist → empty config
    thermark_with_config(&path)
        .arg("info")
        .assert()
        .failure()
        .stderr(predicate::str::contains("config set"));
}
