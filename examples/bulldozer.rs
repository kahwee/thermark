//! Draws a cute line-art bulldozer sticker sized for a 50x30 mm label.
//!
//! ```sh
//! cargo run --example bulldozer -- local/prints/bulldozer.png
//! thermark print -i local/prints/bulldozer.png --label 50x30 --no-fill -d 4
//! ```
//!
//! Draws on the full canvas. `thermark print` places the result inside the
//! printable area, so there is no need to pre-inset the artwork here.
//!
//! Deliberately outlines rather than fills: large solid areas bleed on thermal
//! paper, drain the battery, and read as a blob instead of a drawing.

use image::{GrayImage, Luma};
use thermark::geometry::{LabelMm, SafeArea};

const BLACK: Luma<u8> = Luma([0]);

fn sdf_round_box(px: f32, py: f32, cx: f32, cy: f32, bx: f32, by: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - bx + r;
    let qy = (py - cy).abs() - by + r;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

fn sdf_circle(px: f32, py: f32, cx: f32, cy: f32, r: f32) -> f32 {
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r
}

fn paint(img: &mut GrayImage, f: impl Fn(f32, f32) -> bool) {
    let (w, h) = img.dimensions();
    for y in 0..h {
        for x in 0..w {
            if f(x as f32 + 0.5, y as f32 + 0.5) {
                img.put_pixel(x, y, BLACK);
            }
        }
    }
}

/// Rounded-rect outline of thickness `t`.
#[allow(clippy::too_many_arguments)]
fn stroke_round_rect(img: &mut GrayImage, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, t: f32) {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (bx, by) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    paint(img, |px, py| {
        sdf_round_box(px, py, cx, cy, bx, by, r).abs() <= t / 2.0
    });
}

fn stroke_circle(img: &mut GrayImage, cx: f32, cy: f32, r: f32, t: f32) {
    paint(img, |px, py| sdf_circle(px, py, cx, cy, r).abs() <= t / 2.0);
}

fn fill_circle(img: &mut GrayImage, cx: f32, cy: f32, r: f32) {
    paint(img, |px, py| sdf_circle(px, py, cx, cy, r) <= 0.0);
}

fn stroke_line(img: &mut GrayImage, x0: f32, y0: f32, x1: f32, y1: f32, t: f32) {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len2 = dx * dx + dy * dy;
    paint(img, |px, py| {
        let s = if len2 == 0.0 {
            0.0
        } else {
            (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0)
        };
        let (nx, ny) = (x0 + s * dx, y0 + s * dy);
        ((px - nx).powi(2) + (py - ny).powi(2)).sqrt() <= t / 2.0
    });
}

/// Lower arc of a circle — a smile.
fn smile(img: &mut GrayImage, cx: f32, cy: f32, r: f32, t: f32) {
    paint(img, |px, py| {
        sdf_circle(px, py, cx, cy, r).abs() <= t / 2.0 && py > cy + r * 0.25
    });
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "local/prints/bulldozer.png".to_string());

    // Bottom edge clips on this hardware, so the artwork sits inside the
    // measured safe area rather than centred on the raw canvas.
    let label = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
    let safe = SafeArea::default();
    let area = safe.content(label).expect("50x30 has a printable area");
    let (w, h) = (label.width_px, label.height_px);
    debug_assert!(area.y + area.h <= h);
    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    let t = 5.0; // stroke weight — thin lines close up on thermal paper

    // ── Tracks ──────────────────────────────────────────────────────────
    stroke_round_rect(&mut img, 96.0, 142.0, 344.0, 202.0, 30.0, t);
    for (cx, r) in [(132.0, 17.0), (220.0, 13.0), (308.0, 17.0)] {
        stroke_circle(&mut img, cx, 172.0, r, t);
        fill_circle(&mut img, cx, 172.0, 4.0);
    }
    // Tread ticks along the top run of the track
    for i in 0..9 {
        let x = 118.0 + i as f32 * 26.0;
        stroke_line(&mut img, x, 142.0, x, 153.0, 3.0);
    }

    // ── Body ────────────────────────────────────────────────────────────
    // Outlined, not filled: large solid areas bleed on thermal paper, drain
    // the battery, and read as a blob rather than a drawing.
    stroke_round_rect(&mut img, 120.0, 96.0, 332.0, 148.0, 14.0, t);
    // Side panel detail
    stroke_round_rect(&mut img, 136.0, 108.0, 196.0, 136.0, 6.0, 3.0);
    for i in 0..3 {
        let x = 148.0 + i as f32 * 16.0;
        stroke_line(&mut img, x, 112.0, x, 132.0, 3.0);
    }

    // ── Cab with a window ───────────────────────────────────────────────
    stroke_round_rect(&mut img, 214.0, 34.0, 324.0, 100.0, 16.0, t);
    stroke_round_rect(&mut img, 230.0, 46.0, 308.0, 86.0, 10.0, 3.0);
    // Face
    fill_circle(&mut img, 252.0, 61.0, 5.5);
    fill_circle(&mut img, 286.0, 61.0, 5.5);
    smile(&mut img, 269.0, 61.0, 16.0, 4.0);

    // ── Blade: upright plate + curved lower lip ─────────────────────────
    stroke_round_rect(&mut img, 34.0, 80.0, 70.0, 186.0, 14.0, t);
    stroke_round_rect(&mut img, 34.0, 166.0, 104.0, 196.0, 14.0, t);
    // Ribs on the blade face
    for i in 0..2 {
        let y = 106.0 + i as f32 * 34.0;
        stroke_line(&mut img, 44.0, y, 60.0, y, 3.0);
    }
    // Push arm
    stroke_line(&mut img, 68.0, 118.0, 124.0, 136.0, 6.0);

    // ── Exhaust + puffs ─────────────────────────────────────────────────
    stroke_round_rect(&mut img, 150.0, 58.0, 168.0, 98.0, 8.0, t);
    stroke_circle(&mut img, 158.0, 44.0, 11.0, t);
    stroke_circle(&mut img, 178.0, 24.0, 8.0, 4.0);
    stroke_circle(&mut img, 196.0, 12.0, 5.5, 3.0);

    if let Some(dir) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    img.save(&out).unwrap();
    println!("wrote {out} ({w}x{h})");
}
