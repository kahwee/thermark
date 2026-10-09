use super::*;

#[test]
fn installation_preview_is_scannable_without_creating_config() {
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().expect("temporary installation workspace");
    let config = dir.path().join("config.json");
    let font = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf");
    fixture
        .command_with_config(&config)
        .current_dir(dir.path())
        .args([
            "qr",
            "--url",
            "https://example.com",
            "--text",
            "Hello, label!",
            "--model",
            "b1",
            "--label",
            "50x30",
            "--save",
            "hello-label.png",
            "--no-print",
            "--font",
        ])
        .arg(font)
        .assert()
        .success();

    let gray = image::open(dir.path().join("hello-label.png"))
        .expect("read first-run preview")
        .to_luma8();
    assert_eq!(gray.dimensions(), (384, 240));
    let mut decoder = quircs::Quirc::default();
    let codes: Vec<_> = decoder
        .identify(gray.width() as usize, gray.height() as usize, gray.as_raw())
        .map(|code| {
            code.expect("detect QR")
                .decode()
                .expect("decode QR")
                .payload
        })
        .collect();
    assert_eq!(codes, vec![b"https://example.com".to_vec()]);
    assert!(
        !config.exists(),
        "offline preview must not save printer config"
    );
}

#[test]
fn invalid_font_sizes_are_rejected_for_all_sticker_commands() {
    let fixture = CliFixture::new();
    let dir = &fixture.dir;
    let config = fixture.config();
    let output = dir.path().join("label.png");
    for command in [
        vec!["text", "--text", "HELLO"],
        vec!["qr", "--url", "https://example.com", "--text", "HELLO"],
        vec!["wifi", "--ssid", "Demo-Guest", "--security", "nopass"],
    ] {
        for size in ["NaN", "inf", "-inf", "0", "-1"] {
            fixture
                .command()
                .args(&command)
                .arg(format!("--font-size={size}"))
                .args(["--no-print", "--save"])
                .arg(&output)
                .assert()
                .failure()
                .stderr(predicate::str::contains("finite, positive"));
            assert!(!output.exists());
            assert!(!config.exists());
        }
    }
}

#[test]
fn text_overflow_fails_without_saving_a_partial_label() {
    let fixture = CliFixture::new();
    let dir = &fixture.dir;
    let config = fixture.config();
    let output = dir.path().join("label.png");
    let font = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf");
    fixture
        .command()
        .args(["text", "--text"])
        .arg("OVERFLOW\n".repeat(10))
        .args(["--font-size", "96", "--no-print", "--font"])
        .arg(&font)
        .arg("--save")
        .arg(&output)
        .assert()
        .failure()
        .stderr(predicate::str::contains("text does not fit"));
    assert!(!output.exists());
    assert!(!config.exists());
}

/// Exercise the documented first-run preview from an empty working directory.
/// A vendored font makes the output portable even on a minimal CI image.
#[test]
fn experimental_model_can_render_preview_without_hardware_opt_in() {
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().unwrap();
    let preview = dir.path().join("d110-preview.png");
    let cfg = dir.path().join("config.json");

    fixture
        .command_with_config(&cfg)
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
    let fixture = CliFixture::new();
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
        fixture
            .command_with_config(&cfg)
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
fn wifi_demo_renders_without_print() {
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("wifi.png");
    fixture
        .command()
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
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("wifi-env.png");
    fixture
        .command()
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
    let fixture = CliFixture::new();
    let fixtures_out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("_should_not_write_wifi.png");
    fixture
        .command()
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
fn wifi_open_network_renders_without_password() {
    let fixture = CliFixture::new();
    let cfg_dir = &fixture.dir;
    let with_env = cfg_dir.path().join("open-with-env.png");
    let without_env = cfg_dir.path().join("open-without-env.png");
    let sentinel = "OPEN-NETWORK-PASSWORD-MUST-BE-IGNORED";

    fixture
        .command()
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

    fixture
        .command()
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
fn generated_label_without_save_does_not_announce_temp_file() {
    let fixture = CliFixture::new();
    fixture
        .command()
        .args(["text", "--text", "HELLO", "--no-print"])
        .assert()
        .success()
        .stdout(predicate::str::contains("saved").not());
}

#[test]
fn task_override_does_not_change_physical_profile_geometry() {
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().unwrap();
    let preview = dir.path().join("preview.png");
    let sticker = dir.path().join("sticker.png");

    fixture
        .command()
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

    fixture
        .command()
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
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("threshold-input.png");
    let preview = dir.path().join("threshold-preview.png");
    let cfg = dir.path().join("config.json");
    image::GrayImage::from_raw(4, 1, vec![0, 127, 128, 255])
        .unwrap()
        .save(&input)
        .unwrap();

    fixture
        .command_with_config(&cfg)
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
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("threshold-trim-input.png");
    let preview = dir.path().join("threshold-trim-preview.png");
    let cfg = dir.path().join("config.json");
    image::GrayImage::from_raw(7, 1, vec![255, 204, 205, 0, 205, 204, 255])
        .unwrap()
        .save(&input)
        .unwrap();

    fixture
        .command_with_config(&cfg)
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
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("alpha-input.png");
    let preview = dir.path().join("alpha-preview.png");
    let cfg = dir.path().join("config.json");
    image::RgbaImage::from_raw(2, 1, vec![0, 0, 0, 0, 0, 0, 0, 255])
        .unwrap()
        .save(&input)
        .unwrap();

    fixture
        .command_with_config(&cfg)
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
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("dither-input.png");
    let preview = dir.path().join("dither-preview.png");
    let cfg = dir.path().join("config.json");
    let source = image::GrayImage::from_fn(17, 11, |x, y| {
        image::Luma([((x * 31 + y * 47 + x * y) % 256) as u8])
    });
    source.save(&input).unwrap();

    fixture
        .command_with_config(&cfg)
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
    let fixture = CliFixture::new();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("too-wide.png");
    let preview = dir.path().join("too-wide-preview.png");
    let cfg = dir.path().join("config.json");
    image::GrayImage::from_pixel(385, 1, image::Luma([0]))
        .save(&input)
        .unwrap();

    fixture
        .command_with_config(&cfg)
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
fn sticker_save_errors_are_reported_without_success_or_config_changes() {
    let fixture = CliFixture::new();
    let output = fixture.dir.path().join("occupied.png");
    std::fs::create_dir(&output).unwrap();
    let font = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf");
    for args in [
        vec!["text", "--text", "HELLO"],
        vec!["qr", "--url", "https://example.com", "--text", "HELLO"],
        vec!["wifi", "--ssid", "Demo-Guest", "--security", "nopass"],
    ] {
        fixture
            .command()
            .args(args)
            .args(["--no-print", "--font"])
            .arg(&font)
            .arg("--save")
            .arg(&output)
            .assert()
            .failure()
            .stderr(predicate::str::contains("save "))
            .stderr(predicate::str::contains("occupied.png"))
            .stdout(predicate::str::contains("saved ").not())
            .stdout(predicate::str::contains("OK").not());
        assert!(output.is_dir());
        assert!(!fixture.config().exists());
    }
}
