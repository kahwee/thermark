//! Product demos under `fixtures/` — Wi‑Fi, URL, inventory, name, calibrate.
//!
//! Personal one-off prints (art, real Wi‑Fi) belong in **`local/`** (gitignored),
//! not here. Every file in `fixtures/` must be listed below.
//!
//! ```text
//! cargo test --test fixtures_readme
//! cargo test --test fixtures_readme regenerate_calibrate -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use image::GenericImageView;
use thermark::geometry::LabelMm;
use thermark::image_encode;
use thermark::label::{QrLabelOptions, TextSide, make_qr_label_opts};
use thermark::wifi::{WifiLabelOptions, WifiSecurity, make_wifi_label};

const W: u32 = 384;
const H: u32 = 240;
const MAX_W: u32 = 384;
const TEST_FONT: &str = "tests/fonts/DejaVuSans.ttf";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn test_font() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(TEST_FONT)
}

#[derive(Clone, Copy)]
struct Fixture {
    name: &'static str,
    purpose: &'static str,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Calibrate,
    QrSticker,
}

/// Product fixtures only (committed). Keep in sync with README.
fn curated() -> &'static [Fixture] {
    &[
        Fixture {
            name: "sticker_wifi.png",
            purpose: "Guest Wi‑Fi demo (fake SSID/password only)",
            kind: Kind::QrSticker,
        },
        Fixture {
            name: "sticker_link.png",
            purpose: "Package / share URL QR + text",
            kind: Kind::QrSticker,
        },
        Fixture {
            name: "sticker_inventory.png",
            purpose: "Bin / inventory QR + dense text",
            kind: Kind::QrSticker,
        },
        Fixture {
            name: "sticker_name.png",
            purpose: "Name badge QR + identity lines",
            kind: Kind::QrSticker,
        },
        Fixture {
            name: "sticker_calibrate.png",
            purpose: "Full-bleed geometry / print-area check",
            kind: Kind::Calibrate,
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
    let r = image_encode::encode_path(&path, MAX_W, 127, false)
        .unwrap_or_else(|e| panic!("encode {name}: {e}"));
    assert_eq!((r.width(), r.height()), (W, H), "{name} encoded size");
    let logical_rows: u32 = r
        .rows()
        .iter()
        .map(|packet| match packet.cmd {
            0x84 => u32::from(packet.data[2]),
            0x85 => u32::from(packet.data[5]),
            other => panic!("{name}: unexpected row command {other:#04x}"),
        })
        .sum();
    assert_eq!(logical_rows, H, "{name} encoded logical rows");
    assert!(
        r.rows().len() < H as usize,
        "{name} should coalesce at least one repeated row"
    );
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
            "orphan in fixtures/: {name} — move personal prints to local/ (gitignored) \
             or add to curated()"
        );
    }
    for f in curated() {
        assert!(
            on_disk.iter().any(|n| n == f.name),
            "curated fixture missing: {} ({})",
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
        assert_eq!(
            (w, h),
            (W, H),
            "{} must be full 50×30 canvas, got {w}x{h}",
            f.name
        );
        assert_encodes(f.name);

        let gray = img.to_luma8();
        let (dark, light, frac) = ink_stats(&gray);
        assert!(
            dark > 0 && light > 0,
            "{} blank? dark={dark} light={light}",
            f.name
        );

        match f.kind {
            Kind::Calibrate => {
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
                assert!(
                    (0.08..0.55).contains(&frac),
                    "{} dark fraction {frac:.3} unrealistic for QR sticker",
                    f.name
                );
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
        }
    }
}

#[test]
fn wifi_demo_layout_api_matches_canvas() {
    let lp = LabelMm::parse("50x30").unwrap().to_pixels(MAX_W, 8.0);
    let img = make_wifi_label(&WifiLabelOptions {
        ssid: "Demo-Guest".into(),
        password: "demo-not-real".into(),
        security: WifiSecurity::Wpa,
        hidden: false,
        show_password: false,
        label: lp,
        safe: thermark::geometry::SafeArea::default(),
        text_side: TextSide::Right,
        font_path: Some(test_font()),
        font_name: None,
        font_size: None,
        border: false,
    })
    .expect("render Wi-Fi demo with vendored font");
    assert_eq!(img.dimensions(), (W, H));
    let fix = open_gray("sticker_wifi.png");
    assert_eq!(fix.dimensions(), (W, H));
}

#[test]
fn link_sticker_layout_is_full_canvas_square_qr() {
    let lp = LabelMm::parse("50x30").unwrap().to_pixels(MAX_W, 8.0);
    let img = make_qr_label_opts(&QrLabelOptions {
        url: "https://example.com/o/1042".into(),
        side_text: "ORDER #1042\nShip by Fri\nPriority".into(),
        label: lp,
        safe: thermark::geometry::SafeArea::default(),
        text_side: TextSide::Right,
        border: false,
        font_path: Some(test_font()),
        font_name: None,
        font_size: None,
    })
    .expect("render link sticker with vendored font");
    assert_eq!(img.dimensions(), (W, H));
    let side = thermark::label::max_qr_side(lp, thermark::geometry::SafeArea::default());
    assert_eq!(side, side.min(lp.height_px));
    let fix = open_gray("sticker_link.png");
    assert_eq!(fix.dimensions(), (W, H));
}

#[test]
fn inventory_sticker_dense_type_still_has_quiet_margin() {
    let gray = open_gray("sticker_inventory.png");
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

#[tokio::test]
async fn wifi_fixture_mock_print_streams_rows() {
    use thermark::geometry::LabelMm;
    use thermark::mock::MockTransport;
    use thermark::printer::{PrintOptions, PrinterClient};
    use thermark::protocol::Model;
    use thermark::types::Density;

    let path = fixtures_dir().join("sticker_wifi.png");
    let mut c = PrinterClient::new(MockTransport::new(), Model::B1)
        .with_pacing(thermark::printer::Pacing::INSTANT);
    c.print_image_file_opts(
        &path,
        PrintOptions {
            density: Density::DARK,
            label: Some(LabelMm::parse("50x30").unwrap()),
            fill: false,
            margin_px: 0,
            dither: false,
            ..Default::default()
        },
    )
    .await
    .expect("mock print wifi fixture");

    let cmds = c.transport().tx_cmds();
    assert!(cmds.contains(&0x01), "print start: {cmds:?}");
    let row_packets: Vec<_> = c
        .transport()
        .tx_packets
        .iter()
        .filter(|packet| matches!(packet.cmd, 0x84 | 0x85))
        .collect();
    let logical_rows: u32 = row_packets
        .iter()
        .map(|packet| match packet.cmd {
            0x84 => u32::from(packet.data[2]),
            0x85 => u32::from(packet.data[5]),
            _ => unreachable!(),
        })
        .sum();
    assert_eq!(logical_rows, H);
    assert!(row_packets.len() < H as usize, "rows were not coalesced");
}

/// ```text
/// cargo test --test fixtures_readme regenerate_calibrate -- --ignored --nocapture
/// ```
#[test]
#[ignore = "writes fixtures/sticker_calibrate.png"]
fn regenerate_calibrate() {
    let lp = LabelMm::parse("50x30").unwrap().to_pixels(MAX_W, 8.0);
    let img = image_encode::calibration_pattern(lp, None, 8.0);
    let path = fixtures_dir().join("sticker_calibrate.png");
    img.save(&path).unwrap();
    println!("wrote {}", path.display());
}
