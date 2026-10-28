use super::*;
use std::fs;

fn font() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf")
}

#[test]
fn csv_preview_handles_quoted_multiline_text_and_scannable_qrs_without_config() {
    let fixture = CliFixture::new();
    let csv = fixture.dir.path().join("labels.csv");
    let out = fixture.dir.path().join("preview space's $literal");
    fs::write(&csv, "url,text\r\nhttps://example.com/a,\"BIN A3\nCables, adapters\"\r\nhttps://example.com/b,\"ALEX \"\"AJ\"\"\"\r\n").unwrap();
    fixture
        .command()
        .args(["batch", "--csv"])
        .arg(&csv)
        .arg("--preview-dir")
        .arg(&out)
        .arg("--font")
        .arg(font())
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing printed"))
        .stdout(predicate::str::contains(format!(
            "--job '{}'",
            out.display().to_string().replace('\'', "'\\''")
        )));
    for (number, expected) in [(1, "https://example.com/a"), (2, "https://example.com/b")] {
        let image = image::open(out.join(format!("label-{number:05}.png")))
            .unwrap()
            .to_luma8();
        assert_eq!(image.dimensions(), (384, 240));
        assert!(image.as_raw().iter().all(|v| matches!(v, 0 | 255)));
        let mut decoder = quircs::Quirc::default();
        let codes: Vec<_> = decoder
            .identify(
                image.width() as usize,
                image.height() as usize,
                image.as_raw(),
            )
            .map(|code| code.unwrap().decode().unwrap().payload)
            .collect();
        assert_eq!(codes, vec![expected.as_bytes().to_vec()]);
    }
    assert!(!fixture.config().exists());
    assert!(!out.join("journal.jsonl").exists());
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["total"], 2);
}

#[test]
fn late_invalid_row_does_not_leave_partial_previews_or_connect() {
    let fixture = CliFixture::new();
    let csv = fixture.dir.path().join("labels.csv");
    let out = fixture.dir.path().join("preview");
    for contents in [
        "text\nGOOD\n\"\"\n",
        "text,url\nGOOD,https://example.com\nBAD,\n",
        "text\nGOOD\nBAD,EXTRA\n",
    ] {
        fs::write(&csv, contents).unwrap();
        fixture
            .command()
            .args(["batch", "--csv"])
            .arg(&csv)
            .arg("--preview-dir")
            .arg(&out)
            .arg("--print")
            .assert()
            .failure()
            .stderr(predicate::str::contains("row 2"));
        assert!(!out.exists());
        assert!(!fixture.config().exists());
    }
    fs::write(
        &csv,
        format!("text\nGOOD\n\"{}\"\n", "OVERFLOW\n".repeat(15)),
    )
    .unwrap();
    fixture
        .command()
        .args(["batch", "--csv"])
        .arg(&csv)
        .arg("--preview-dir")
        .arg(&out)
        .args(["--font-size", "96", "--font"])
        .arg(font())
        .assert()
        .failure()
        .stderr(predicate::str::contains("row 2"));
    assert!(!out.exists());
}

#[test]
fn reusable_template_resolves_relative_font_and_cli_overrides() {
    let fixture = CliFixture::new();
    let csv = fixture.dir.path().join("labels.csv");
    let template = fixture.dir.path().join("layout.json");
    fs::copy(font(), fixture.dir.path().join("font.ttf")).unwrap();
    fs::write(&csv, "text\nSAVED LAYOUT\n").unwrap();
    fs::write(&template, r#"{"kind":"text","label":"40x20","font":"font.ttf","align":"left","border":true,"density":3}"#).unwrap();
    for (name, extra, size) in [
        ("template", vec![], (320, 160)),
        ("override", vec!["--label", "50x30"], (384, 240)),
    ] {
        let out = fixture.dir.path().join(name);
        fixture
            .command()
            .args(["batch", "--csv"])
            .arg(&csv)
            .arg("--template")
            .arg(&template)
            .arg("--preview-dir")
            .arg(&out)
            .args(extra)
            .assert()
            .success();
        assert_eq!(
            image::open(out.join("label-00001.png"))
                .unwrap()
                .to_luma8()
                .dimensions(),
            size
        );
    }
}

#[test]
fn batch_rejects_bad_headers_templates_ranges_and_output_reuse() {
    let fixture = CliFixture::new();
    let csv = fixture.dir.path().join("labels.csv");
    let out = fixture.dir.path().join("preview");
    for contents in [
        "text,text\nA,B\n",
        "text,ur1\nA,B\n",
        "url\nhttps://example.com\n",
        "text\n",
    ] {
        fs::write(&csv, contents).unwrap();
        fixture
            .command()
            .args(["batch", "--csv"])
            .arg(&csv)
            .arg("--preview-dir")
            .arg(&out)
            .assert()
            .failure();
        assert!(!out.exists());
    }
    fs::write(&csv, "text\nGOOD\n").unwrap();
    for start in ["0", "2"] {
        fixture
            .command()
            .args(["batch", "--csv"])
            .arg(&csv)
            .arg("--preview-dir")
            .arg(&out)
            .args(["--start-at", start])
            .assert()
            .failure();
        assert!(!out.exists());
    }
    let template = fixture.dir.path().join("bad.json");
    for contents in [
        r#"{"kind":"qr"}"#,
        r#"{"lable":"50x30"}"#,
        r#"{"font_size":0}"#,
        r#"{"density":6}"#,
        r#"{"align":"sideways"}"#,
    ] {
        fs::write(&template, contents).unwrap();
        fixture
            .command()
            .args(["batch", "--csv"])
            .arg(&csv)
            .arg("--preview-dir")
            .arg(&out)
            .arg("--template")
            .arg(&template)
            .assert()
            .failure();
        assert!(!out.exists());
    }
    fs::create_dir(&out).unwrap();
    fs::write(out.join("keep.txt"), "KEEP").unwrap();
    fixture
        .command()
        .args(["batch", "--csv"])
        .arg(&csv)
        .arg("--preview-dir")
        .arg(&out)
        .arg("--font")
        .arg(font())
        .assert()
        .failure()
        .stderr(predicate::str::contains("fresh preview directory"));
    assert_eq!(fs::read_to_string(out.join("keep.txt")).unwrap(), "KEEP");
}

#[test]
fn setup_requires_media_or_terminal_and_invalid_media_preserves_config() {
    let fixture = CliFixture::new();
    let original = r#"{"addr":"B1-Saved","label":"50x30","model":"b1"}"#;
    fs::write(fixture.config(), original).unwrap();
    fixture
        .command()
        .arg("setup")
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs a terminal"));
    fixture
        .command()
        .args(["setup", "--label", "NaNx30"])
        .assert()
        .failure();
    assert_eq!(fs::read_to_string(fixture.config()).unwrap(), original);
}

#[test]
fn batch_limits_rows_but_spools_pages_above_the_old_memory_limit() {
    let fixture = CliFixture::new();
    let csv = fixture.dir.path().join("labels.csv");
    let out = fixture.dir.path().join("preview");
    fs::write(&csv, format!("text\n{}", "LABEL\n".repeat(1001))).unwrap();
    fixture
        .command()
        .args(["batch", "--csv"])
        .arg(&csv)
        .arg("--preview-dir")
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("exceeds 1000 labels"));
    fs::write(&csv, format!("text\n{}", "LABEL\n".repeat(750))).unwrap();
    fixture
        .command()
        .args(["batch", "--csv"])
        .arg(&csv)
        .arg("--preview-dir")
        .arg(&out)
        .args(["--label", "50x30"])
        .assert()
        .success();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["total"], 750);
    assert!(out.join("label-00750.png").exists());
}

#[test]
fn shipped_inventory_example_renders_and_all_three_qrs_decode() {
    let fixture = CliFixture::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = fixture.dir.path().join("inventory");
    fixture
        .command()
        .args(["batch", "--csv"])
        .arg(root.join("examples/batch/inventory.csv"))
        .arg("--template")
        .arg(root.join("examples/batch/inventory.json"))
        .arg("--preview-dir")
        .arg(&out)
        .arg("--font")
        .arg(font())
        .assert()
        .success();
    for row in 1..=3 {
        let image = image::open(out.join(format!("label-{row:05}.png")))
            .unwrap()
            .to_luma8();
        let mut decoder = quircs::Quirc::default();
        let payloads: Vec<_> = decoder
            .identify(
                image.width() as usize,
                image.height() as usize,
                image.as_raw(),
            )
            .map(|code| code.unwrap().decode().unwrap().payload)
            .collect();
        assert_eq!(
            payloads,
            vec![format!("https://example.com/bins/A{row}").into_bytes()]
        );
    }
}

#[test]
fn prepared_job_verify_and_status_work_without_configuration_or_printer() {
    let fixture = CliFixture::new();
    let csv = fixture.dir.path().join("labels.csv");
    let out = fixture.dir.path().join("job");
    fs::write(&csv, "text,url\nBIN A,https://example.com/a\n").unwrap();
    fixture
        .command()
        .args(["batch", "--csv"])
        .arg(csv)
        .arg("--preview-dir")
        .arg(&out)
        .assert()
        .success();
    // Offline job inspection must ignore a malformed saved printer configuration.
    fs::write(fixture.config(), "malformed").unwrap();
    fixture
        .command()
        .args(["job", "verify", "--job"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("Verified 1 pages"));
    let output = fixture
        .command()
        .args(["job", "status", "--job"])
        .arg(&out)
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(report["rows"][0]["outcome"].is_null());
    fs::write(out.join("label-00001.png"), b"changed").unwrap();
    fixture
        .command()
        .args(["job", "verify", "--job"])
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("hash mismatch"));
}
