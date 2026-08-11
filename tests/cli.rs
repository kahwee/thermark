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
fn scan_help_mentions_save() {
    thermark()
        .args(["scan", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("save"))
        .stdout(predicate::str::contains("name"));
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
fn experimental_task_requires_allow_flag() {
    let (_dir, cfg) = temp_config_path();
    thermark_with_config(&cfg)
        .args([
            "print",
            "-a",
            "B1-Fake",
            "-i",
            // Product smoke fixture (guest Wi‑Fi demo)
            "fixtures/sticker_wifi.png",
            "--task",
            "simple",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("allow-experimental"));
}

#[test]
fn experimental_model_default_requires_allow_flag() {
    let (_dir, cfg) = temp_config_path();
    thermark_with_config(&cfg)
        .args([
            "print",
            "-a",
            "B1-Fake",
            "-i",
            "fixtures/sticker_wifi.png",
            "--model",
            "b21",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("allow-experimental"));
}

#[test]
fn print_help_mentions_allow_experimental() {
    thermark()
        .args(["print", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("allow-experimental"));
}

#[test]
fn print_help_mentions_dither_and_no_fill() {
    thermark()
        .args(["print", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dither"))
        .stdout(predicate::str::contains("no-fill"));
}

/// Product smoke fixture must exist — CLI print path dependency.
#[test]
fn wifi_fixture_exists_for_print_smoke() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/sticker_wifi.png");
    assert!(
        p.is_file(),
        "product smoke fixture missing: {} (print tests depend on it)",
        p.display()
    );
    let meta = std::fs::metadata(&p).expect("stat wifi fixture");
    assert!(meta.len() > 500, "wifi fixture suspiciously small");
}

#[test]
fn wifi_help_mentions_ssid() {
    thermark()
        .args(["wifi", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ssid"))
        .stdout(predicate::str::contains("password"));
}

#[test]
fn wifi_demo_renders_without_print() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("wifi.png");
    let (_cfg_dir, cfg) = temp_config_path();
    thermark_with_config(&cfg)
        .args([
            "wifi",
            "--ssid",
            "Demo-Guest",
            "--password",
            "demo-not-real",
            "--label",
            "50x30",
            "--font-name",
            "helvetica",
            "--save",
            out.to_str().unwrap(),
            "--no-print",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Demo-Guest"));
    assert!(out.is_file(), "wifi PNG not written");
}

#[test]
fn wifi_password_from_env_without_flag() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("wifi-env.png");
    let (_cfg_dir, cfg) = temp_config_path();
    thermark_with_config(&cfg)
        .env("THERMARK_WIFI_PASSWORD", "from-env-secret")
        .args([
            "wifi",
            "--ssid",
            "EnvNet",
            "--label",
            "50x30",
            "--font-name",
            "helvetica",
            "--save",
            out.to_str().unwrap(),
            "--no-print",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("EnvNet"));
    assert!(out.is_file());
}

#[test]
fn wifi_refuses_save_under_fixtures() {
    let (_cfg_dir, cfg) = temp_config_path();
    let fixtures_out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("_should_not_write_wifi.png");
    thermark_with_config(&cfg)
        .args([
            "wifi",
            "--ssid",
            "Nope",
            "--password",
            "secret",
            "--label",
            "50x30",
            "--save",
            fixtures_out.to_str().unwrap(),
            "--no-print",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("fixtures"));
    assert!(
        !fixtures_out.exists(),
        "must not write Wi‑Fi sticker into fixtures/"
    );
}

#[test]
fn wifi_missing_password_errors_helpfully() {
    let (_cfg_dir, cfg) = temp_config_path();
    thermark_with_config(&cfg)
        .env_remove("THERMARK_WIFI_PASSWORD")
        .args(["wifi", "--ssid", "NoPass", "--label", "50x30", "--no-print"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("THERMARK_WIFI_PASSWORD"));
}

#[test]
fn print_help_mentions_fuzzy_ble_match() {
    thermark()
        .args(["print", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fuzzy"));
}

#[test]
fn doctor_help_mentions_fuzzy() {
    thermark()
        .args(["doctor", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fuzzy"));
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
fn malformed_config_is_reported_and_not_overwritten() {
    let (_dir, path) = temp_config_path();
    let original = b"{ definitely not json\n";
    std::fs::write(&path, original).unwrap();

    thermark_with_config(&path)
        .args(["config", "set", "-a", "B1-New"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("parse config"));

    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn generated_label_without_save_does_not_announce_temp_file() {
    let (_dir, cfg) = temp_config_path();
    thermark_with_config(&cfg)
        .args(["text", "--text", "HELLO", "--no-print"])
        .assert()
        .success()
        .stdout(predicate::str::contains("saved").not());
}

#[test]
fn mismatched_task_limits_preview_and_sticker_canvas_before_printing() {
    let dir = tempfile::tempdir().unwrap();
    let preview = dir.path().join("preview.png");
    let sticker = dir.path().join("sticker.png");
    let (_cfg_dir, cfg) = temp_config_path();

    thermark_with_config(&cfg)
        .args([
            "print",
            "-i",
            "fixtures/sticker_wifi.png",
            "--label",
            "50x30",
            "--task",
            "d110",
            "--allow-experimental",
            "--preview",
            preview.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(image::open(&preview).unwrap().width(), 96);

    thermark_with_config(&cfg)
        .args([
            "text",
            "--text",
            "NARROW",
            "--label",
            "50x30",
            "--task",
            "d110",
            "--allow-experimental",
            "--save",
            sticker.to_str().unwrap(),
            "--no-print",
        ])
        .assert()
        .success();
    assert_eq!(image::open(&sticker).unwrap().width(), 96);
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
