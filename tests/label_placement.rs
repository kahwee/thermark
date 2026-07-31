//! Every path that puts content on a label must keep ink inside the printable
//! area. These bugs were found one at a time by printing; this covers them all
//! at once, without hardware.

use thermark::geometry::{LabelMm, LabelPx, SafeArea};
use thermark::image_encode::ink_bounds;
use thermark::label::{
    QrLabelOptions, TextAlign, TextLabelOptions, TextSide, make_qr_label_opts, make_text_label,
};
use thermark::wifi::{WifiLabelOptions, WifiSecurity, make_wifi_label};

fn label() -> LabelPx {
    LabelMm::parse("50x30").unwrap().to_pixels(384)
}

/// Rows/cols of ink, and whether any lands outside the printable band.
fn assert_inside_safe_area(img: &image::GrayImage, lp: LabelPx, safe: SafeArea, what: &str) {
    let (w, h) = img.dimensions();
    assert_eq!((w, h), (lp.width_px, lp.height_px), "{what}: wrong canvas");
    let ink = ink_bounds(img, 127).unwrap_or_else(|| panic!("{what}: rendered nothing"));
    let bottom = ink.y + ink.h - 1;
    assert!(
        ink.y >= safe.top,
        "{what}: ink at row {} is above the printable area (top inset {})",
        ink.y,
        safe.top
    );
    assert!(
        bottom < h - safe.bottom,
        "{what}: ink at row {bottom} is in the unprintable feed band (starts {})",
        h - safe.bottom
    );
}

#[test]
fn qr_label_stays_inside_the_printable_area() {
    let lp = label();
    let safe = SafeArea::default();
    let img = make_qr_label_opts(&QrLabelOptions {
        url: "https://example.com/a/rather/long/url/for/density".into(),
        side_text: "ORDER 1042\nShip Friday\nPriority".into(),
        label: lp,
        safe,
        text_side: TextSide::Right,
        border: false,
        font_path: None,
        font_name: None,
        font_size: None,
    })
    .expect("qr label");
    assert_inside_safe_area(&img, lp, safe, "qr");
}

#[test]
fn text_label_stays_inside_the_printable_area() {
    let lp = label();
    let safe = SafeArea::default();
    let Ok(img) = make_text_label(&TextLabelOptions {
        text: "THERMARK\nbulldozer crew\n#1".into(),
        label: lp,
        safe,
        align: TextAlign::Center,
        border: false,
        font_path: None,
        font_name: None,
        font_size: None,
    }) else {
        return; // no system font on this host
    };
    assert_inside_safe_area(&img, lp, safe, "text");
}

#[test]
fn wifi_label_stays_inside_the_printable_area() {
    let lp = label();
    let safe = SafeArea::default();
    let Ok(img) = make_wifi_label(&WifiLabelOptions {
        ssid: "Cafe-Guest".into(),
        password: "s3cret-password".into(),
        security: WifiSecurity::Wpa,
        hidden: false,
        show_password: false,
        label: lp,
        safe,
        text_side: TextSide::Right,
        font_path: None,
        font_name: None,
        font_size: None,
        border: false,
    }) else {
        return;
    };
    assert_inside_safe_area(&img, lp, safe, "wifi");
}

#[test]
fn text_is_optically_centred_in_the_printable_area() {
    let lp = label();
    let safe = SafeArea::default();
    let Ok(img) = make_text_label(&TextLabelOptions {
        text: "THERMARK\nbulldozer crew\n#1".into(),
        label: lp,
        safe,
        align: TextAlign::Center,
        border: false,
        font_path: None,
        font_name: None,
        font_size: None,
    }) else {
        return;
    };

    let ink = ink_bounds(&img, 127).expect("rendered nothing");
    let above = ink.y - safe.top;
    let below = (lp.height_px - safe.bottom) - (ink.y + ink.h - 1);
    // Centring on font metrics left 27 above and 1 below, because `ascent`
    // reserves space above cap height that the glyphs never use.
    assert!(
        above.abs_diff(below) <= 4,
        "text is not optically centred: {above} above, {below} below"
    );
}

#[test]
fn trimmed_artwork_stays_inside_the_printable_area() {
    use thermark::image_encode::{contain_label, fill_label, trim_white};
    let lp = label();
    let safe = SafeArea::default();

    // Art whose ink touches every edge of its own canvas — the worst case for
    // placement, and what trimming produces.
    let mut g = image::GrayImage::from_pixel(384, 240, image::Luma([255]));
    for y in 0..240 {
        for x in 0..384 {
            // Solid block: ink touches every edge, so placement has nowhere to
            // hide a mistake.
            g.put_pixel(x, y, image::Luma([0]));
        }
    }
    let art = trim_white(image::DynamicImage::ImageLuma8(g), 127);

    for (mode, placed) in [
        ("contain", contain_label(art.clone(), lp, safe, 0)),
        ("fill", fill_label(art, lp, safe, 0)),
    ] {
        assert_inside_safe_area(&placed.to_luma8(), lp, safe, mode);
    }
}

/// Every media size the user actually stocks must render, not just 50x30.
///
/// The whole layout stack is derived from `LabelPx`, but two things were still
/// hardcoded to the starter roll and only this test caught them: the boundary
/// probe's millimetre range, and the assumption that a QR always has a text
/// column beside it.
#[test]
fn common_media_sizes_all_render_inside_the_printable_area() {
    let safe = SafeArea::default();
    for spec in ["40x20", "40x30", "50x30", "50x80"] {
        let lp = LabelMm::parse(spec).unwrap().to_pixels(384);

        let text = make_text_label(&TextLabelOptions {
            text: "PANTRY\nrice, dried".into(),
            label: lp,
            safe,
            align: TextAlign::Center,
            border: false,
            font_path: None,
            font_name: None,
            font_size: None,
        })
        .unwrap_or_else(|e| panic!("{spec}: text label failed: {e}"));
        assert_inside_safe_area(&text, lp, safe, &format!("{spec} text"));

        let qr = make_qr_label_opts(&QrLabelOptions {
            url: "https://example.com/inventory/10428".into(),
            side_text: "BIN 12".into(),
            label: lp,
            safe,
            text_side: TextSide::Right,
            border: false,
            font_path: None,
            font_name: None,
            font_size: None,
        })
        .unwrap_or_else(|e| panic!("{spec}: qr label failed: {e}"));
        assert_inside_safe_area(&qr, lp, safe, &format!("{spec} qr"));
    }
}

/// The boundary probe exists to find where the printer stops on *this* media,
/// so its bars must sit against that media's trailing edge.
///
/// It used to mark a fixed 17..29 mm: three bars on a 20 mm label, and on
/// 50x80 a staircase across the middle of the label measuring nothing.
#[test]
fn boundary_probe_marks_the_trailing_edge_of_any_media() {
    use thermark::label::{boundary_range, make_boundary_label};

    for spec in ["40x20", "40x30", "50x30", "50x80"] {
        let lp = LabelMm::parse(spec).unwrap().to_pixels(384);
        let range = boundary_range(lp);
        let height_mm = lp.height_px / 8;

        assert_eq!(
            *range.end(),
            height_mm - 1,
            "{spec}: last bar must be the final drawable millimetre"
        );
        assert!(
            range.start() < range.end(),
            "{spec}: probe needs more than one bar to be readable"
        );

        let img = make_boundary_label(lp, None).unwrap_or_else(|e| panic!("{spec}: {e}"));
        let ink = ink_bounds(&img, 127).unwrap_or_else(|| panic!("{spec}: probe drew nothing"));
        let bottom = ink.y + ink.h - 1;
        // Full bleed on purpose — the probe measures the edge, so it must draw
        // right up to it.
        assert!(
            bottom >= lp.height_px - 8,
            "{spec}: probe stops at row {bottom}, short of the edge at {}",
            lp.height_px - 1
        );
    }
}
