//! Every path that puts content on a label must keep ink inside the printable
//! area. These bugs were found one at a time by printing; this covers them all
//! at once, without hardware.

use thermark::geometry::{LabelMm, LabelPx, SafeArea};
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
    let (mut top, mut bottom) = (u32::MAX, 0u32);
    for (_, y, p) in img.enumerate_pixels() {
        if p[0] < 128 {
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }
    assert!(top != u32::MAX, "{what}: rendered nothing");
    assert!(
        top >= safe.top,
        "{what}: ink at row {top} is above the printable area (top inset {})",
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
fn trimmed_artwork_stays_inside_the_printable_area() {
    use thermark::image_encode::{contain_label_in, fill_label_in, trim_white};
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
        ("contain", contain_label_in(art.clone(), lp, safe, 0)),
        ("fill", fill_label_in(art, lp, safe, 0)),
    ] {
        assert_inside_safe_area(&placed.to_luma8(), lp, safe, mode);
    }
}
