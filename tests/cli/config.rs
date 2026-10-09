use super::*;

#[test]
fn config_help_lists_subcommands() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("clear"));
}

#[test]
fn offline_commands_ignore_malformed_config() {
    let fixture = CliFixture::new();
    let path = fixture.config();
    std::fs::write(&path, "{ definitely not json\n").unwrap();

    for args in [vec!["tasks"], vec!["encode", "1a", "01"]] {
        fixture.command().args(args).assert().success();
    }
}

#[test]
fn config_path_does_not_parse_config() {
    let fixture = CliFixture::new();
    let path = fixture.config();
    std::fs::write(&path, "{ definitely not json\n").unwrap();

    fixture
        .command()
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains(path.to_string_lossy().as_ref()));
}

#[test]
fn unverified_b1_usb_path_requires_allow_flag_before_address_resolution() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["print", "-i", "fixtures/sticker_wifi.png", "--conn", "usb"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("allow-experimental"))
        .stderr(predicate::str::contains("over 'usb'"))
        .stderr(predicate::str::contains("no printer address").not());
}

#[cfg(unix)]
#[test]
fn config_rejects_non_unicode_override() {
    let fixture = CliFixture::new();
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"config-\xff.json".to_vec()));
    // Check the read-only command first: a regression must fail before any
    // mutating command could fall back to the developer's real config.
    for args in [
        vec!["config", "path"],
        vec!["config", "show", "--json"],
        vec!["config", "set", "--addr", "B1-TestPrinter"],
        vec!["config", "clear"],
    ] {
        fixture
            .command_with_config(&path)
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "THERMARK_CONFIG must be valid Unicode",
            ));
    }
}

#[test]
fn config_set_show_clear() {
    let fixture = CliFixture::new();
    let path = fixture.config();

    fixture
        .command()
        .args(["config", "set", "-a", "B1-TestPrinter", "--scan-secs", "7"])
        .assert()
        .success()
        .stdout(predicate::str::contains("B1-TestPrinter"));

    assert!(path.exists(), "config.json should be created");
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.trim_start().starts_with('{'), "{body}");
    assert!(body.contains("B1-TestPrinter"));
    assert!(body.contains("scan_secs") || body.contains("\"scan_secs\""));

    fixture
        .command()
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("B1-TestPrinter"))
        .stdout(predicate::str::contains('7'));

    fixture
        .command()
        .args(["config", "show", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("B1-TestPrinter"))
        .stdout(predicate::str::contains('{'));

    fixture
        .command()
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config.json"));

    fixture
        .command()
        .args(["config", "clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));

    assert!(!path.exists());
}

#[test]
fn safe_area_rejects_non_finite_negative_and_consuming_values() {
    let fixture = CliFixture::new();
    let path = fixture.config();
    for args in [
        vec!["config", "safe-area", "--top", "NaN"],
        vec!["config", "safe-area", "--left", "-1"],
        vec!["config", "safe-area", "--last-tick", "31"],
        vec!["config", "safe-area", "--top", "20", "--bottom", "10"],
    ] {
        fixture.command().args(args).assert().failure();
    }
    assert!(!path.exists(), "invalid input must not create config.json");
}

#[test]
fn config_set_partially_merges_addr_connection_and_label() {
    let fixture = CliFixture::new();
    let path = fixture.config();

    fixture
        .command()
        .args(["config", "set", "--conn", "usb"])
        .assert()
        .success();

    fixture
        .command()
        .args(["config", "set", "--addr", "B1-A", "--model", "b21pro"])
        .assert()
        .success();

    fixture
        .command()
        .args(["config", "set", "--label", "40x20"])
        .assert()
        .success();

    fixture
        .command()
        .args(["config", "set", "--addr", "B1-B"])
        .assert()
        .success();

    let cfg = thermark::config::Config::load_from(&path).unwrap();
    assert_eq!(cfg.addr.as_deref(), Some("B1-B"));
    assert_eq!(cfg.connection, Some(thermark::config::ConnPref::Usb));
    assert_eq!(cfg.model, Some(thermark::protocol::Model::B21Pro));
    assert_eq!(cfg.label.as_deref(), Some("40x20"));
}

#[test]
fn config_set_rejects_no_updates() {
    let fixture = CliFixture::new();
    let path = fixture.config();

    fixture
        .command()
        .args(["config", "set"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no config updates provided"));

    assert!(
        !path.exists(),
        "an empty update must not create config.json"
    );
}

#[test]
fn config_show_empty() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no config file yet"));
}

#[test]
fn info_without_addr_errors_helpfully() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .arg("info")
        .assert()
        .failure()
        .stderr(predicate::str::contains("config set"));
}

#[test]
fn malformed_config_is_reported_and_not_overwritten() {
    let fixture = CliFixture::new();
    let path = fixture.config();
    let original = b"{ definitely not json\n";
    std::fs::write(&path, original).unwrap();

    fixture
        .command()
        .args(["config", "set", "-a", "B1-New"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("parse config"));

    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn doctor_use_config_without_saved_addr_fails() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["doctor", "--use-config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("config set"));
}

#[test]
fn thermark_addr_env_used_when_no_flag() {
    let fixture = CliFixture::new();
    let assert = fixture
        .command()
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
