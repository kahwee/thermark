//! Golden-image tests: every renderer must produce the same pixels it did when
//! its golden was accepted.
//!
//! Placement bugs in this project were historically found by printing a label
//! and photographing it. These catch the same class of change on `cargo test`,
//! with no printer and no wasted media.
//!
//! ```sh
//! cargo test --test golden                     # verify
//! UPDATE_GOLDEN=1 cargo test --test golden      # accept current output
//! ```
//!
//! Review the diff when regenerating: a changed golden is either a fix you
//! meant to make or a regression you did not.
//!
//! Text cases render with a **vendored** font (`tests/fonts/DejaVuSans.ttf`),
//! not a system one. They used to pass `font_path: None` and take whatever the
//! host offered: Helvetica on macOS, DejaVu on Linux CI. The goldens were
//! generated on macOS, so every text case failed on CI from the day this
//! harness landed — a permanent red that hid real regressions. Pinning the font
//! makes the bytes identical everywhere.

use image::GrayImage;
use std::path::PathBuf;
use thermark::geometry::{LabelMm, LabelPx, SafeArea};
use thermark::image_encode::{calibration_pattern, contain_label, fill_label, trim_white};
use thermark::label::{
    QrLabelOptions, TextAlign, TextLabelOptions, TextSide, make_boundary_label,
    make_calibration_label, make_qr_label_opts, make_text_label,
};
use thermark::wifi::{WifiLabelOptions, WifiSecurity, make_wifi_label};

const GOLDEN_DIR: &str = "tests/golden";
/// Vendored so text rasterises identically on every host. Never swap this for a
/// system font: the goldens are pixel comparisons, and a different face changes
/// every one of them.
const GOLDEN_FONT: &str = "tests/fonts/DejaVuSans.ttf";

fn golden_font() -> Option<PathBuf> {
    let p = PathBuf::from(GOLDEN_FONT);
    p.exists().then_some(p)
}
/// Where a mismatching render is written, so it can be inspected.
const ACTUAL_DIR: &str = "target/golden-actual";

fn label() -> LabelPx {
    LabelMm::parse("50x30").unwrap().to_pixels(384)
}

/// Deterministic stand-in for user artwork: a shape with its own white margin,
/// which is what makes trimming and placement interesting.
fn artwork() -> image::DynamicImage {
    let mut g = GrayImage::from_pixel(384, 240, image::Luma([255]));
    for y in 40..170 {
        for x in 60..300 {
            let edge = !(46..164).contains(&y) || !(66..294).contains(&x);
            if edge {
                g.put_pixel(x, y, image::Luma([0]));
            }
        }
    }
    for i in 0..60 {
        g.put_pixel(70 + i, 60 + i, image::Luma([0]));
    }
    image::DynamicImage::ImageLuma8(g)
}

/// A render under test. `None` means "cannot run here" (missing system font).
struct Case {
    name: &'static str,
    render: fn() -> Option<GrayImage>,
}

fn cases() -> Vec<Case> {
    vec![
        // ── Geometry: no font needed, always runs ────────────────────────
        Case {
            name: "calibration_pattern_50x30",
            render: || Some(calibration_pattern(label(), Some(SafeArea::default()))),
        },
        Case {
            name: "calibration_pattern_full_bleed",
            render: || Some(calibration_pattern(label(), None)),
        },
        Case {
            name: "art_contain_safe",
            render: || Some(contain_label(artwork(), label(), SafeArea::default(), 0).to_luma8()),
        },
        Case {
            name: "art_fill_safe",
            render: || Some(fill_label(artwork(), label(), SafeArea::default(), 0).to_luma8()),
        },
        Case {
            name: "art_trimmed_contain",
            render: || {
                let art = trim_white(artwork(), 127);
                Some(contain_label(art, label(), SafeArea::default(), 0).to_luma8())
            },
        },
        Case {
            name: "art_full_bleed",
            render: || Some(contain_label(artwork(), label(), SafeArea::NONE, 0).to_luma8()),
        },
        // ── Font-dependent ───────────────────────────────────────────────
        Case {
            name: "qr_50x30",
            render: || {
                make_qr_label_opts(&QrLabelOptions {
                    url: "https://github.com/kahwee/thermark".into(),
                    side_text: "ORDER 1042\nShip Friday\nPriority".into(),
                    label: label(),
                    safe: SafeArea::default(),
                    text_side: TextSide::Right,
                    border: false,
                    font_path: golden_font(),
                    font_name: None,
                    font_size: None,
                })
                .ok()
            },
        },
        Case {
            name: "qr_text_left",
            render: || {
                make_qr_label_opts(&QrLabelOptions {
                    url: "https://example.com".into(),
                    side_text: "LEFT\nSIDE".into(),
                    label: label(),
                    safe: SafeArea::default(),
                    text_side: TextSide::Left,
                    border: true,
                    font_path: golden_font(),
                    font_name: None,
                    font_size: None,
                })
                .ok()
            },
        },
        Case {
            name: "text_centered",
            render: || {
                make_text_label(&TextLabelOptions {
                    text: "THERMARK\nbulldozer crew\n#1".into(),
                    label: label(),
                    safe: SafeArea::default(),
                    align: TextAlign::Center,
                    border: false,
                    font_path: golden_font(),
                    font_name: None,
                    font_size: None,
                })
                .ok()
            },
        },
        Case {
            name: "text_left_small",
            render: || {
                make_text_label(&TextLabelOptions {
                    text: "SKU-00421\nshelf B3\nqty 12".into(),
                    label: label(),
                    safe: SafeArea::default(),
                    align: TextAlign::Left,
                    border: false,
                    font_path: golden_font(),
                    font_name: None,
                    font_size: Some(14.0),
                })
                .ok()
            },
        },
        Case {
            name: "text_long_wraps_whole_words",
            render: || {
                make_text_label(&TextLabelOptions {
                    text: "SUPERCALIFRAGILISTIC handling instructions".into(),
                    label: label(),
                    safe: SafeArea::default(),
                    align: TextAlign::Center,
                    border: false,
                    font_path: golden_font(),
                    font_name: None,
                    font_size: None,
                })
                .ok()
            },
        },
        Case {
            name: "wifi_50x30",
            render: || {
                make_wifi_label(&WifiLabelOptions {
                    ssid: "Cafe-Guest".into(),
                    password: "s3cret-password".into(),
                    security: WifiSecurity::Wpa,
                    hidden: false,
                    show_password: false,
                    label: label(),
                    safe: SafeArea::default(),
                    text_side: TextSide::Right,
                    font_path: golden_font(),
                    font_name: None,
                    font_size: None,
                    border: false,
                })
                .ok()
            },
        },
        Case {
            name: "calibration_numbered",
            render: || {
                make_calibration_label(label(), SafeArea::default(), golden_font().as_deref()).ok()
            },
        },
        Case {
            name: "boundary_probe",
            render: || make_boundary_label(label(), golden_font().as_deref()).ok(),
        },
    ]
}

/// Pixel difference, plus the first differing coordinate for diagnosis.
fn diff(actual: &GrayImage, expected: &GrayImage) -> Option<(u64, u32, u32)> {
    let mut count = 0u64;
    let mut first = None;
    for (x, y, p) in actual.enumerate_pixels() {
        if expected.get_pixel(x, y)[0] != p[0] {
            count += 1;
            first.get_or_insert((x, y));
        }
    }
    first.map(|(x, y)| (count, x, y))
}

#[test]
fn golden_renders_are_unchanged() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    if update {
        std::fs::create_dir_all(GOLDEN_DIR).expect("create golden dir");
    }

    // Without this the text cases fall back to `font_path: None`, silently
    // pick up a system font, and compare host-specific rasterisation against
    // committed goldens — which is exactly how this suite stayed red on CI.
    assert!(
        golden_font().is_some(),
        "vendored font missing: {GOLDEN_FONT}\n\
         Text goldens are pixel comparisons and MUST NOT fall back to a system \
         font. Restore the file rather than regenerating the goldens."
    );

    let mut verified = Vec::new();
    let mut skipped = Vec::new();
    let mut failures = Vec::new();

    for case in cases() {
        let Some(actual) = (case.render)() else {
            skipped.push(case.name);
            continue;
        };
        let path = PathBuf::from(GOLDEN_DIR).join(format!("{}.png", case.name));

        if update {
            actual.save(&path).expect("write golden");
            verified.push(case.name);
            continue;
        }

        let Ok(expected) = image::open(&path) else {
            failures.push(format!(
                "{}: no golden at {} — create it with `UPDATE_GOLDEN=1 cargo test --test golden`",
                case.name,
                path.display()
            ));
            continue;
        };
        let expected = expected.to_luma8();

        if actual.dimensions() != expected.dimensions() {
            failures.push(format!(
                "{}: size {:?} != golden {:?}",
                case.name,
                actual.dimensions(),
                expected.dimensions()
            ));
            continue;
        }

        if let Some((count, x, y)) = diff(&actual, &expected) {
            // Write the render out so the change can be looked at, not guessed at.
            let _ = std::fs::create_dir_all(ACTUAL_DIR);
            let out = PathBuf::from(ACTUAL_DIR).join(format!("{}.png", case.name));
            let _ = actual.save(&out);
            failures.push(format!(
                "{}: {count} px differ, first at ({x},{y}); actual written to {}",
                case.name,
                out.display()
            ));
        } else {
            verified.push(case.name);
        }
    }

    if update {
        println!("regenerated {} golden(s): {verified:?}", verified.len());
        return;
    }

    if !skipped.is_empty() {
        println!("skipped (no system font): {skipped:?}");
    }
    println!("verified {} golden(s)", verified.len());

    assert!(
        failures.is_empty(),
        "golden renders changed:\n  {}\n\nIf intended, review the images then run \
         `UPDATE_GOLDEN=1 cargo test --test golden`.",
        failures.join("\n  ")
    );
}

/// The geometry cases need no font, so a bare checkout must still cover them.
#[test]
fn geometry_cases_never_skip() {
    let font_free = [
        "calibration_pattern_50x30",
        "calibration_pattern_full_bleed",
        "art_contain_safe",
        "art_fill_safe",
        "art_trimmed_contain",
        "art_full_bleed",
    ];
    for name in font_free {
        let case = cases()
            .into_iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no case named {name}"));
        assert!(
            (case.render)().is_some(),
            "{name} must render without a system font"
        );
    }
}
