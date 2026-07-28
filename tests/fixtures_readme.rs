//! Curated sticker fixtures under `fixtures/` — docs + boundary smoke tests.
//!
//! These match real pocket-label jobs (link share, inventory, name badge, calibrate,
//! photo sticker). Every file in `fixtures/` must be listed here.
//!
//! Regenerate PNGs (needs system fonts):
//! ```text
//! cargo build --release
//! ./target/release/thermark qr ... --save fixtures/sticker_link.png --no-print
//! cargo test --test fixtures_readme regenerate_calibrate -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use image::GenericImageView;
use thermark::geometry::LabelMm;
use thermark::image_encode;
use thermark::label::{QrLabelOptions, TextSide, make_qr_label_opts};

const W: u32 = 384;
const H: u32 = 240;
const MAX_W: u32 = 384;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

#[derive(Clone, Copy)]
struct Fixture {
    name: &'static str,
    /// Human purpose (README / product story).
    purpose: &'static str,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Full-bleed geometry pattern — ink near edges.
    Calibrate,
    /// QR + text sticker (scannable label).
    QrSticker,
    /// Photograph source (JPEG); print path uses contain + dither.
    Photo,
}

/// Canonical fixture set. Keep in sync with README and `fixtures/` on disk.
fn curated() -> &'static [Fixture] {
    &[
        Fixture {
            name: "sticker_link.png",
            purpose: "Package / share link: QR + order-style text",
            kind: Kind::QrSticker,
        },
        Fixture {
            name: "sticker_inventory.png",
            purpose: "Bin / inventory tag: QR + dense multi-line type",
            kind: Kind::QrSticker,
        },
        Fixture {
            name: "sticker_name.png",
            purpose: "Name / desk badge: QR + short identity lines",
            kind: Kind::QrSticker,
        },
        Fixture {
            name: "sticker_calibrate.png",
            purpose: "Geometry: full-bleed border/diagonals/cross (print area check)",
            kind: Kind::Calibrate,
        },
        Fixture {
            name: "photo_sticker.jpg",
            purpose: "Photo sticker source (Unsplash); use --no-fill --margin --dither",
            kind: Kind::Photo,
        },
    ]
}

fn open_gray(name: &str) -> image::GrayImage {
    let path = fixtures_dir().join(name);
    assert!(
        path.is_file(),
        "missing fixture {name} at {}",
        path.display()
    );
    image::open(&path)
        .unwrap_or_else(|e| panic!("open {name}: {e}"))
        .to_luma8()
}

fn ink_stats(gray: &image::GrayImage) -> (usize, usize, f64) {
    let total = gray.width() as usize * gray.height() as usize;
    let dark = gray.pixels().filter(|p| p[0] < 128).count();
    let light = total - dark;
    let dark_frac = dark as f64 / total as f64;
    (dark, light, dark_frac)
}

fn assert_encodes(name: &str) {
    let path = fixtures_dir().join(name);
    let (w, h, packets) = image_encode::encode_image_path(&path, MAX_W, 0, 127)
        .unwrap_or_else(|e| panic!("encode {name}: {e}"));
    assert_eq!((w, h), (W, H), "{name} encoded size");
    assert_eq!(packets.len() as u32, H, "{name} one packet per row");
}

#[test]
fn fixtures_dir_matches_curated_list() {
    let dir = fixtures_dir();
    assert!(dir.is_dir(), "fixtures/ missing");

    let listed: HashSet<&str> = curated().iter().map(|f| f.name).collect();
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .expect("read fixtures/")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    on_disk.sort();

    for name in &on_disk {
        assert!(
            listed.contains(name.as_str()),
            "orphan in fixtures/: {name} — delete or add to curated()"
        );
    }
    for f in curated() {
        assert!(
            on_disk.iter().any(|n| n == f.name),
            "curated fixture missing on disk: {} ({})",
            f.name,
            f.purpose
        );
    }
}

#[test]
fn all_fixtures_open_and_encode() {
    for f in curated() {
        let path = fixtures_dir().join(f.name);
        let img = image::open(&path).unwrap_or_else(|e| panic!("{}: {e}", f.name));
        let (w, h) = img.dimensions();

        match f.kind {
            Kind::Photo => {
                // Photo source may be larger than the label; print path scales it.
                assert!(w >= 200 && h >= 150, "photo too small: {w}x{h}");
                assert!(
                    path.extension().and_then(|e| e.to_str()) == Some("jpg")
                        || path.extension().and_then(|e| e.to_str()) == Some("jpeg"),
                    "photo fixture should be JPEG"
                );
            }
            Kind::Calibrate | Kind::QrSticker => {
                assert_eq!(
                    (w, h),
                    (W, H),
                    "{} must be full 50×30 canvas ({}×{}), got {w}x{h}",
                    f.name,
                    W,
                    H
                );
                assert_encodes(f.name);
            }
        }

        let gray = img.to_luma8();
        let (dark, light, frac) = ink_stats(&gray);
        assert!(
            dark > 0 && light > 0,
            "{} blank? dark={dark} light={light}",
            f.name
        );

        match f.kind {
            Kind::Calibrate => {
                // Full-bleed: meaningful ink, including near corners (boundary).
                assert!(
                    (0.05..0.45).contains(&frac),
                    "calibrate dark fraction {frac:.3} out of band"
                );
                for &(x, y) in &[(2u32, 2), (W - 3, 2), (2, H - 3), (W - 3, H - 3)] {
                    assert!(
                        gray.get_pixel(x, y)[0] < 128,
                        "calibrate should ink near corner ({x},{y})"
                    );
                }
            }
            Kind::QrSticker => {
                // QR+text: substantial but not solid black.
                assert!(
                    (0.08..0.55).contains(&frac),
                    "{} dark fraction {frac:.3} unrealistic for QR sticker",
                    f.name
                );
                // Left third should be denser (QR modules) than far-right margin strip.
                let mut left_dark = 0usize;
                let mut right_dark = 0usize;
                let mut left_n = 0usize;
                let mut right_n = 0usize;
                for y in 0..H {
                    for x in 0..(W / 3) {
                        left_n += 1;
                        if gray.get_pixel(x, y)[0] < 128 {
                            left_dark += 1;
                        }
                    }
                    for x in (W * 5 / 6)..W {
                        right_n += 1;
                        if gray.get_pixel(x, y)[0] < 128 {
                            right_dark += 1;
                        }
                    }
                }
                let left_f = left_dark as f64 / left_n as f64;
                let right_f = right_dark as f64 / right_n as f64;
                assert!(
                    left_f > right_f + 0.05,
                    "{} expected denser QR on left (left={left_f:.3} right={right_f:.3})",
                    f.name
                );
            }
            Kind::Photo => {
                // Natural photo: mid-tone heavy, not a 1-bit logo.
                assert!((0.05..0.85).contains(&frac), "photo dark frac {frac:.3}");
            }
        }
    }
}

#[test]
fn photo_print_pipeline_centers_with_margin() {
    let path = fixtures_dir().join("photo_sticker.jpg");
    let img = image::open(&path).expect("photo_sticker.jpg");
    let lp = LabelMm::parse("50x30").unwrap().to_pixels(MAX_W);
    let placed = image_encode::contain_label(img, lp, 16);
    assert_eq!(placed.dimensions(), (W, H));
    let gray = placed.to_luma8();
    // Outer margin ring must stay white (no edge bleed from content).
    for x in 0..W {
        assert_eq!(gray.get_pixel(x, 0)[0], 255, "top margin");
        assert_eq!(gray.get_pixel(x, 8)[0], 255, "inner top margin band");
    }
    let bw = image_encode::gray_to_print_bits(&gray, 127, true);
    let dark = bw.pixels().filter(|p| p[0] > 127).count();
    assert!(
        dark > 500,
        "dithered photo should have printable dots ({dark})"
    );
    let (ew, eh, packets) =
        image_encode::encode_image_opts(placed, MAX_W, 0, 127, true).expect("encode photo");
    assert_eq!((ew, eh), (W, H));
    assert_eq!(packets.len() as u32, H);
}

#[test]
fn link_sticker_layout_is_full_canvas_square_qr() {
    // Boundary: layout API produces exact media size + square QR (product contract).
    let lp = LabelMm::parse("50x30").unwrap().to_pixels(MAX_W);
    let img = make_qr_label_opts(&QrLabelOptions {
        url: "https://example.com/o/1042".into(),
        side_text: "ORDER #1042\nShip by Fri\nPriority".into(),
        label: lp,
        text_side: TextSide::Right,
        border: false,
        font_path: None,
        font_name: Some("helvetica".into()),
        font_size: None,
    });
    // Font may be missing on some CI images — only assert when load works.
    if let Ok(img) = img {
        assert_eq!(img.dimensions(), (W, H));
        let side = thermark::label::max_qr_side(lp);
        assert_eq!(side, side.min(lp.height_px), "QR side within height");
        // Fixture on disk should match the same canvas contract.
        let fix = open_gray("sticker_link.png");
        assert_eq!(fix.dimensions(), (W, H));
    }
}

#[test]
fn inventory_sticker_dense_type_still_has_quiet_margin() {
    let gray = open_gray("sticker_inventory.png");
    // Far-right column strip should not be solid black (text column has padding).
    let mut edge_dark = 0usize;
    for y in 0..H {
        if gray.get_pixel(W - 1, y)[0] < 128 {
            edge_dark += 1;
        }
    }
    let edge_f = edge_dark as f64 / H as f64;
    assert!(
        edge_f < 0.35,
        "inventory right edge too dark ({edge_f:.2}) — layout bleeding to edge?"
    );
}

/// Regenerate `fixtures/sticker_calibrate.png` from the geometry helper.
///
/// ```text
/// cargo test --test fixtures_readme regenerate_calibrate -- --ignored --nocapture
/// ```
#[test]
#[ignore = "writes fixtures/sticker_calibrate.png"]
fn regenerate_calibrate() {
    let lp = LabelMm::parse("50x30").unwrap().to_pixels(MAX_W);
    let img = image_encode::calibration_pattern(lp);
    let path = fixtures_dir().join("sticker_calibrate.png");
    img.save(&path).unwrap();
    println!("wrote {}", path.display());
}
