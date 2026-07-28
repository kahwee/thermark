//! Validates the curated README / docs label fixtures under `fixtures/`.
//!
//! These PNGs are the sample labels referenced by README and AGENTS.md.
//! All are full 50×30 mm canvases → **384×240 px**.
//!
//! Regenerate calibration (and re-check paths) with:
//! ```text
//! cargo test --test fixtures_readme regenerate_calibrate_fixture -- --ignored --nocapture
//! ```
//!
//! QR previews are produced by the CLI (see each fixture comment below), e.g.:
//! ```text
//! ./target/release/thermark qr --url "https://example.com" \
//!   --text $'Helvetica\nABC\n123' --font-name helvetica \
//!   --label 50x30 --save fixtures/preview_helvetica.png --no-print
//! ```

use std::path::{Path, PathBuf};

use image::GenericImageView;
use thermark::geometry::LabelMm;
use thermark::image_encode;

/// Expected dimensions for all 50×30 mm fixtures (8 px/mm, width clamped/aligned to 384).
const EXPECT_W: u32 = 384;
const EXPECT_H: u32 = 240;
const MAX_WIDTH: u32 = 384;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Curated fixtures kept in-repo for docs and smoke tests.
///
/// Each entry: `(filename, README / docs mapping)`.
fn curated_fixtures() -> &'static [(&'static str, &'static str)] {
    &[
        // Main README QR preview (Helvetica):
        //   thermark qr --url … --text $'Helvetica\nABC\n123' --font-name helvetica --label 50x30 --save fixtures/preview_helvetica.png --no-print
        (
            "preview_helvetica.png",
            "README primary QR + Helvetica preview",
        ),
        // Times font comparison:
        //   thermark qr … --text $'Times\nABC\n123' --font-name times --label 50x30 --save fixtures/preview_times.png --no-print
        ("preview_times.png", "README Times font preview"),
        // Denser Arial text column:
        //   thermark qr … --text $'ABC\nHELLO\n123' --font-name arial --label 50x30 --save fixtures/qr_arial_label.png --no-print
        ("qr_arial_label.png", "README Arial / denser text QR label"),
        // Fixed small type:
        //   thermark qr … --text $'small type\n…' --font-name helvetica --font-size 11 --label 50x30 --save fixtures/qr_small_type.png --no-print
        ("qr_small_type.png", "README small fixed font-size QR label"),
        // Generic print smoke image (kept as-is; not regenerated here):
        //   thermark print -i fixtures/test_label.png --label 50x30
        ("test_label.png", "README / AGENTS.md print example input"),
        // Calibration pattern from `image_encode::calibration_pattern` (see regenerate test):
        //   thermark calibrate --label 50x30
        (
            "calibrate_50x30.png",
            "README calibrate / geometry check pattern",
        ),
    ]
}

fn assert_fixture_ok(name: &str, purpose: &str) {
    let path = fixtures_dir().join(name);
    assert!(
        path.is_file(),
        "fixture missing: {} ({purpose}) — path {}",
        name,
        path.display()
    );

    let img = image::open(&path).unwrap_or_else(|e| {
        panic!("failed to open fixture {name} ({purpose}): {e}");
    });
    let (w, h) = img.dimensions();
    assert_eq!(
        (w, h),
        (EXPECT_W, EXPECT_H),
        "fixture {name} dimensions {w}x{h}, expected {EXPECT_W}x{EXPECT_H} ({purpose})"
    );

    // Must have both dark (printable) and light pixels so we catch blank/corrupt files.
    let gray = img.to_luma8();
    let mut has_dark = false;
    let mut has_light = false;
    for p in gray.pixels() {
        if p[0] < 128 {
            has_dark = true;
        } else {
            has_light = true;
        }
        if has_dark && has_light {
            break;
        }
    }
    assert!(
        has_dark && has_light,
        "fixture {name} should contain both dark and light pixels (dark={has_dark} light={has_light}); {purpose}"
    );

    let (ew, eh, packets) = image_encode::encode_image_path(&path, MAX_WIDTH, 0, 127)
        .unwrap_or_else(|e| panic!("encode_image_path failed for {name}: {e}"));
    assert_eq!(ew, EXPECT_W, "encoded width for {name}");
    assert_eq!(eh, EXPECT_H, "encoded height for {name}");
    assert_eq!(
        packets.len() as u32,
        EXPECT_H,
        "expected one print row packet per image row for {name}"
    );
}

#[test]
fn all_curated_fixtures_exist_and_encode() {
    for (name, purpose) in curated_fixtures() {
        assert_fixture_ok(name, purpose);
    }
}

#[test]
fn curated_fixture_list_is_complete() {
    // Every PNG left in fixtures/ must be listed (no orphan/redundant files).
    let dir = fixtures_dir();
    assert!(dir.is_dir(), "fixtures dir missing: {}", dir.display());

    let listed: std::collections::HashSet<&str> =
        curated_fixtures().iter().map(|(n, _)| *n).collect();

    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .expect("read fixtures/")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".png"))
        .collect();
    on_disk.sort();

    for name in &on_disk {
        assert!(
            listed.contains(name.as_str()),
            "unexpected PNG in fixtures/: {name} — add to curated_fixtures() or delete it"
        );
    }
    for (name, _) in curated_fixtures() {
        assert!(
            on_disk.iter().any(|n| n == name),
            "curated fixture not on disk: {name}"
        );
    }
}

/// One-shot generator for `fixtures/calibrate_50x30.png`.
///
/// ```text
/// cargo test --test fixtures_readme regenerate_calibrate_fixture -- --ignored --nocapture
/// ```
#[test]
#[ignore = "run with: cargo test --test fixtures_readme regenerate_calibrate_fixture -- --ignored --nocapture"]
fn regenerate_calibrate_fixture() {
    let lp = LabelMm::parse("50x30").unwrap().to_pixels(MAX_WIDTH);
    assert_eq!((lp.width_px, lp.height_px), (EXPECT_W, EXPECT_H));
    let img = image_encode::calibration_pattern(lp);
    let path = fixtures_dir().join("calibrate_50x30.png");
    img.save(&path)
        .unwrap_or_else(|e| panic!("save {}: {e}", path.display()));
    println!("wrote {}", path.display());
}
