use super::*;

#[test]
fn help_exits_zero() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("thermal label"));
}

#[test]
fn scan_help_mentions_save() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["scan", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("save"))
        .stdout(predicate::str::contains("name"));
}

#[test]
fn tasks_prints_profile_registry() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .arg("tasks")
        .assert()
        .success()
        .stdout(predicate::str::contains("B1 Pro"))
        .stdout(predicate::str::contains("B21 Pro"))
        .stdout(predicate::str::contains("B18"))
        .stdout(predicate::str::contains("D11_H"))
        .stdout(predicate::str::contains("D110"))
        .stdout(predicate::str::contains("tested"))
        .stdout(predicate::str::contains("experimental"))
        .stdout(predicate::str::contains("unresolved"))
        .stdout(predicate::str::contains("Any path except B1+b1 over BLE"));
}

#[test]
fn encode_rfid_probe() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["encode", "1a", "01"])
        .assert()
        .success()
        .stdout(predicate::str::contains("55551a01011aaaaa"));
}

#[test]
fn bad_model_rejected() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args([
            "print",
            "-i",
            "fixtures/sticker_wifi.png",
            "--model",
            "not-a-model",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("possible values"));
}

#[test]
fn bad_task_rejected() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["print", "-a", "x", "-i", "nope.png", "--task", "not-a-task"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("possible values"));
}

#[test]
fn experimental_task_requires_allow_flag() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args([
            "print",
            "-a",
            "B1-Fake",
            "-i",
            // Product smoke fixture (guest Wi‑Fi demo)
            "fixtures/sticker_wifi.png",
            "--task",
            "d110",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("allow-experimental"));
}

#[test]
fn experimental_model_default_requires_allow_flag() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args([
            "print",
            "-i",
            "fixtures/sticker_wifi.png",
            "--model",
            "b21pro",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("allow-experimental"))
        .stderr(predicate::str::contains("no printer address").not());
}

#[test]
fn experimental_model_with_b1_task_requires_allow_flag_before_connecting() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args([
            "print",
            "-i",
            "fixtures/sticker_wifi.png",
            "--model",
            "b21pro",
            "--task",
            "b1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("allow-experimental"))
        .stderr(predicate::str::contains("no printer address").not());
}

#[test]
fn print_help_mentions_allow_experimental() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["print", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("allow-experimental"));
}

#[test]
fn identify_help_offers_json_hardware_capture() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["identify", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("machine-readable"));
}

#[test]
fn print_help_mentions_dither_and_no_fill() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["print", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dither"))
        .stdout(predicate::str::contains("no-fill"));
}

/// Product smoke fixture must exist — CLI print path dependency.
#[test]
fn wifi_help_mentions_ssid() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["wifi", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ssid"))
        .stdout(predicate::str::contains("password"));
}

#[test]
fn wifi_missing_password_errors_helpfully() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .env_remove("THERMARK_WIFI_PASSWORD")
        .args(["wifi", "--ssid", "NoPass", "--label", "50x30", "--no-print"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("THERMARK_WIFI_PASSWORD"));
}

#[test]
fn print_help_mentions_fuzzy_ble_match() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["print", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fuzzy"));
}

#[test]
fn doctor_help_mentions_fuzzy() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["doctor", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fuzzy"))
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("privacy-safe"));
}

#[test]
fn fonts_runs() {
    let fixture = CliFixture::new();
    fixture.command().arg("fonts").assert().success();
}

#[test]
fn doctor_host_only_runs() {
    let fixture = CliFixture::new();
    let assert = fixture.command().arg("doctor").assert();
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
fn doctor_json_is_machine_readable_and_privacy_labeled() {
    let fixture = CliFixture::new();
    let assert = fixture.command().args(["doctor", "--json"]).assert();
    let output = assert.get_output();
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "status {:?}",
        output.status
    );

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(
        report["privacy"],
        "printer identifiers and local paths omitted"
    );
    assert!(report["checks"].is_array());
}

#[test]
fn scan_durations_are_bounded_at_the_cli() {
    let fixture = CliFixture::new();
    for args in [
        vec!["scan", "--seconds", "0"],
        vec!["doctor", "--seconds", "301"],
        vec!["config", "set", "-a", "B1-Test", "--scan-secs", "0"],
        vec!["info", "-a", "B1-Test", "--scan-secs", "999999"],
    ] {
        fixture
            .command()
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "scan time must be between 1 and 300 seconds",
            ));
    }
}
