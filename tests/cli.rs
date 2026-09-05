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
fn tasks_prints_profile_registry() {
    thermark()
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
    thermark()
        .args(["encode", "1a", "01"])
        .assert()
        .success()
        .stdout(predicate::str::contains("55551a01011aaaaa"));
}

#[test]
fn offline_commands_ignore_malformed_config() {
    let (_dir, path) = temp_config_path();
    std::fs::write(&path, "{ definitely not json\n").unwrap();

    for args in [vec!["tasks"], vec!["encode", "1a", "01"]] {
        thermark_with_config(&path).args(args).assert().success();
    }
}

#[test]
fn config_path_does_not_parse_config() {
    let (_dir, path) = temp_config_path();
    std::fs::write(&path, "{ definitely not json\n").unwrap();

    thermark_with_config(&path)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains(path.to_string_lossy().as_ref()));
}

#[test]
fn bad_model_rejected() {
    thermark()
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
            "d110",
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
    let (_dir, cfg) = temp_config_path();
    thermark_with_config(&cfg)
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
fn unverified_b1_usb_path_requires_allow_flag_before_address_resolution() {
    let (_dir, cfg) = temp_config_path();
    thermark_with_config(&cfg)
        .args(["print", "-i", "fixtures/sticker_wifi.png", "--conn", "usb"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("allow-experimental"))
        .stderr(predicate::str::contains("over 'usb'"))
        .stderr(predicate::str::contains("no printer address").not());
}

#[test]
fn experimental_model_can_render_preview_without_hardware_opt_in() {
    let dir = tempfile::tempdir().unwrap();
    let preview = dir.path().join("d110-preview.png");
    let cfg = dir.path().join("config.json");

    thermark_with_config(&cfg)
        .args([
            "print",
            "--image",
            "fixtures/sticker_wifi.png",
            "--model",
            "d110",
            "--label",
            "12x30",
            "--preview",
            preview.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("allow-experimental").not());

    assert_eq!(image::open(preview).unwrap().width(), 96);
}

#[test]
fn experimental_model_can_generate_stickers_without_hardware_opt_in() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.json");
    let text = dir.path().join("text.png");
    let qr = dir.path().join("qr.png");
    let wifi = dir.path().join("wifi.png");

    for args in [
        vec![
            "text",
            "--text",
            "OFFLINE",
            "--model",
            "b21pro",
            "--label",
            "40x20",
            "--save",
            text.to_str().unwrap(),
            "--no-print",
        ],
        vec![
            "qr",
            "--url",
            "https://example.com/42",
            "--text",
            "ORDER 42",
            "--model",
            "b21pro",
            "--label",
            "40x20",
            "--save",
            qr.to_str().unwrap(),
            "--no-print",
        ],
        vec![
            "wifi",
            "--ssid",
            "OfflineGuest",
            "--password",
            "not-a-secret",
            "--model",
            "b21pro",
            "--label",
            "40x20",
            "--save",
            wifi.to_str().unwrap(),
            "--no-print",
        ],
    ] {
        thermark_with_config(&cfg)
            .args(args)
            .assert()
            .success()
            .stderr(predicate::str::contains("allow-experimental").not());
    }

    for output in [text, qr, wifi] {
        assert!(output.is_file(), "{} was not generated", output.display());
        assert_eq!(
            image::open(&output).unwrap().to_luma8().dimensions(),
            (472, 236),
            "{} must use the selected B21 Pro 300 dpi profile",
            output.display()
        );
    }
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
fn identify_help_offers_json_hardware_capture() {
    thermark()
        .args(["identify", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("machine-readable"));
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
fn wifi_open_network_renders_without_password() {
    let (cfg_dir, cfg) = temp_config_path();
    let with_env = cfg_dir.path().join("open-with-env.png");
    let without_env = cfg_dir.path().join("open-without-env.png");
    let sentinel = "OPEN-NETWORK-PASSWORD-MUST-BE-IGNORED";

    thermark_with_config(&cfg)
        .env("THERMARK_WIFI_PASSWORD", sentinel)
        .args([
            "wifi",
            "--ssid",
            "Cafe-Guest",
            "--security",
            "nopass",
            "--label",
            "50x30",
            "--show-password",
            "--save",
            with_env.to_str().unwrap(),
            "--no-print",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cafe-Guest"))
        .stdout(predicate::str::contains("open network (no password)"))
        .stdout(predicate::str::contains(sentinel).not())
        .stderr(predicate::str::contains(sentinel).not());

    thermark_with_config(&cfg)
        .env_remove("THERMARK_WIFI_PASSWORD")
        .args([
            "wifi",
            "--ssid",
            "Cafe-Guest",
            "--security",
            "nopass",
            "--label",
            "50x30",
            "--show-password",
            "--save",
            without_env.to_str().unwrap(),
            "--no-print",
        ])
        .assert()
        .success();

    let with_env = image::open(with_env).unwrap().into_luma8();
    let without_env = image::open(without_env).unwrap().into_luma8();
    assert_eq!(with_env.dimensions(), without_env.dimensions());
    assert!(
        with_env.as_raw() == without_env.as_raw(),
        "an open-network label must not render THERMARK_WIFI_PASSWORD"
    );
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

#[cfg(unix)]
#[test]
fn config_rejects_non_unicode_override() {
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
        thermark_with_config(&path)
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
fn scan_durations_are_bounded_at_the_cli() {
    for args in [
        vec!["scan", "--seconds", "0"],
        vec!["doctor", "--seconds", "301"],
        vec!["config", "set", "-a", "B1-Test", "--scan-secs", "0"],
        vec!["info", "-a", "B1-Test", "--scan-secs", "999999"],
    ] {
        thermark()
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "scan time must be between 1 and 300 seconds",
            ));
    }
}

#[test]
fn safe_area_rejects_non_finite_negative_and_consuming_values() {
    let (_dir, path) = temp_config_path();
    for args in [
        vec!["config", "safe-area", "--top", "NaN"],
        vec!["config", "safe-area", "--left", "-1"],
        vec!["config", "safe-area", "--last-tick", "31"],
        vec!["config", "safe-area", "--top", "20", "--bottom", "10"],
    ] {
        thermark_with_config(&path).args(args).assert().failure();
    }
    assert!(!path.exists(), "invalid input must not create config.json");
}

#[test]
fn config_set_partially_merges_addr_connection_and_label() {
    let (_dir, path) = temp_config_path();

    thermark_with_config(&path)
        .args(["config", "set", "--conn", "usb"])
        .assert()
        .success();

    thermark_with_config(&path)
        .args(["config", "set", "--addr", "B1-A", "--model", "b21pro"])
        .assert()
        .success();

    thermark_with_config(&path)
        .args(["config", "set", "--label", "40x20"])
        .assert()
        .success();

    thermark_with_config(&path)
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
    let (_dir, path) = temp_config_path();

    thermark_with_config(&path)
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
fn task_override_does_not_change_physical_profile_geometry() {
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
    assert_eq!(image::open(&preview).unwrap().width(), 384);

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
    assert_eq!(image::open(&sticker).unwrap().width(), 384);
}

#[test]
fn print_preview_shows_hard_threshold_burn_bits_as_black() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("threshold-input.png");
    let preview = dir.path().join("threshold-preview.png");
    let cfg = dir.path().join("config.json");
    image::GrayImage::from_raw(4, 1, vec![0, 127, 128, 255])
        .unwrap()
        .save(&input)
        .unwrap();

    thermark_with_config(&cfg)
        .args([
            "print",
            "--image",
            input.to_str().unwrap(),
            "--threshold",
            "127",
            "--no-trim",
            "--preview",
            preview.to_str().unwrap(),
        ])
        .assert()
        .success();

    let pixels = image::open(preview).unwrap().into_luma8().into_raw();
    assert_eq!(pixels, [0, 0, 255, 255]);
}

#[test]
fn print_preview_trim_preserves_pixels_burned_at_a_non_default_threshold() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("threshold-trim-input.png");
    let preview = dir.path().join("threshold-trim-preview.png");
    let cfg = dir.path().join("config.json");
    image::GrayImage::from_raw(7, 1, vec![255, 204, 205, 0, 205, 204, 255])
        .unwrap()
        .save(&input)
        .unwrap();

    thermark_with_config(&cfg)
        .args([
            "print",
            "--image",
            input.to_str().unwrap(),
            "--threshold",
            "50",
            "--preview",
            preview.to_str().unwrap(),
        ])
        .assert()
        .success();

    let preview = image::open(preview).unwrap().into_luma8();
    assert_eq!(preview.dimensions(), (5, 1));
    assert_eq!(preview.as_raw(), &[0, 255, 0, 255, 0]);
}

#[test]
fn print_preview_composites_transparent_pixels_on_white() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("alpha-input.png");
    let preview = dir.path().join("alpha-preview.png");
    let cfg = dir.path().join("config.json");
    image::RgbaImage::from_raw(2, 1, vec![0, 0, 0, 0, 0, 0, 0, 255])
        .unwrap()
        .save(&input)
        .unwrap();

    thermark_with_config(&cfg)
        .args([
            "print",
            "--image",
            input.to_str().unwrap(),
            "--no-trim",
            "--preview",
            preview.to_str().unwrap(),
        ])
        .assert()
        .success();

    let pixels = image::open(preview).unwrap().into_luma8().into_raw();
    assert_eq!(pixels, [255, 0]);
}

#[test]
fn print_preview_matches_encoder_dither_bits_and_has_binary_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("dither-input.png");
    let preview = dir.path().join("dither-preview.png");
    let cfg = dir.path().join("config.json");
    let source = image::GrayImage::from_fn(17, 11, |x, y| {
        image::Luma([((x * 31 + y * 47 + x * y) % 256) as u8])
    });
    source.save(&input).unwrap();

    thermark_with_config(&cfg)
        .args([
            "print",
            "--image",
            input.to_str().unwrap(),
            "--threshold",
            "103",
            "--dither",
            "--no-trim",
            "--preview",
            preview.to_str().unwrap(),
        ])
        .assert()
        .success();

    let actual = image::open(preview).unwrap().into_luma8();
    let mut expected = thermark::image_encode::gray_to_print_bits(&source, 103, true);
    for pixel in expected.pixels_mut() {
        pixel[0] = 255 - pixel[0];
    }
    assert_eq!(actual, expected);
    assert!(actual.pixels().all(|pixel| matches!(pixel[0], 0 | 255)));
    assert!(actual.pixels().any(|pixel| pixel[0] == 0));
    assert!(actual.pixels().any(|pixel| pixel[0] == 255));
}

#[test]
fn print_preview_rejects_dimensions_the_encoder_cannot_send() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("too-wide.png");
    let preview = dir.path().join("too-wide-preview.png");
    let cfg = dir.path().join("config.json");
    image::GrayImage::from_pixel(385, 1, image::Luma([0]))
        .save(&input)
        .unwrap();

    thermark_with_config(&cfg)
        .args([
            "print",
            "--image",
            input.to_str().unwrap(),
            "--no-trim",
            "--preview",
            preview.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("exceeds printer max"));

    assert!(!preview.exists());
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
