//! Offline calibration drawing.

use crate::geometry::{LabelPx, SafeArea};
use image::{GrayImage, Luma};

/// Spacing between calibration rings, in px (0.5 mm at 8 px/mm).
pub const CALIBRATION_RING_STEP_PX: u32 = 4;
/// How many rings the calibration pattern draws.
pub const CALIBRATION_RINGS: u32 = 6;
/// Length of a major (5 mm) feed-ruler tick, in px. Numerals are placed clear
/// of this — see [`crate::label::make_calibration_label`].
pub const CALIBRATION_RULER_MAJOR_PX: u32 = 26;
/// Length of a minor (1 mm) feed-ruler tick, in px.
pub const CALIBRATION_RULER_MINOR_PX: u32 = 12;

/// Calibration pattern: concentric rings at known insets, plus diagonals and a
/// centre cross.
///
/// Ring *k* (counting inward from 0) sits `k * CALIBRATION_RING_STEP_PX` from
/// the edge. Print it, count how many rings came out **complete on all four
/// sides**, and the first complete ring's inset is the safe margin for that
/// media. A single border only tells you *that* something clipped; the rings
/// tell you *how much*.
/// Additionally outlines `safe` as a thick rectangle.
///
/// The thick box is the pass/fail test: if it prints complete on all four
/// sides, the configured [`SafeArea`] is inside the real printable region and
/// labels will not clip. The thin rings around it measure how much headroom
/// (or shortfall) there is.
pub fn calibration_pattern(
    label: LabelPx,
    safe: Option<SafeArea>,
    pixels_per_mm: f64,
) -> GrayImage {
    let w = label.width_px;
    let h = label.height_px;
    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    if w == 0 || h == 0 {
        return img;
    }

    // Diagonals + centre cross: reveal skew and vertical centring.
    for y in 0..h {
        for x in 0..w {
            let expect_down = (y as i64 * (w as i64 - 1)) / (h as i64 - 1).max(1);
            let expect_up = ((h as i64 - 1 - y as i64) * (w as i64 - 1)) / (h as i64 - 1).max(1);
            let on_diag = (x as i64 - expect_down).abs() <= 1 || (x as i64 - expect_up).abs() <= 1;
            let on_cross =
                (x as i64 - w as i64 / 2).abs() <= 1 || (y as i64 - h as i64 / 2).abs() <= 1;
            if on_diag || on_cross {
                img.put_pixel(x, y, Luma([0]));
            }
        }
    }

    // Concentric rings, 1px each so a clipped ring is unambiguous.
    let ring_step = (0.5 * pixels_per_mm).round().max(1.0) as u32;
    for ring in 0..CALIBRATION_RINGS {
        let inset = ring * ring_step;
        if inset * 2 + 1 >= w.min(h) {
            break;
        }
        let (x0, y0) = (inset, inset);
        let (x1, y1) = (w - 1 - inset, h - 1 - inset);
        for x in x0..=x1 {
            img.put_pixel(x, y0, Luma([0]));
            img.put_pixel(x, y1, Luma([0]));
        }
        for y in y0..=y1 {
            img.put_pixel(x0, y, Luma([0]));
            img.put_pixel(x1, y, Luma([0]));
        }
    }

    // Feed ruler down both sides: a minor tick every 1 mm, a long major tick
    // every 5 mm. Read off where the print stops to get the exact loss at the
    // feed edge — the rings only resolve 0.5 mm near the very edge.
    let ruler_scale = pixels_per_mm / crate::geometry::PX_PER_MM;
    let major_len = (f64::from(CALIBRATION_RULER_MAJOR_PX) * ruler_scale).round() as u32;
    let minor_len = (f64::from(CALIBRATION_RULER_MINOR_PX) * ruler_scale).round() as u32;
    let height_mm = (f64::from(h) / pixels_per_mm).floor() as u32;
    for mm in 0..=height_mm {
        let y = (f64::from(mm) * pixels_per_mm).round() as u32;
        if y >= h {
            break;
        }
        let major = mm % 5 == 0;
        let len = if major { major_len } else { minor_len };
        let thick = if major { 3 } else { 1 };
        for t in 0..thick {
            let yy = (y + t).min(h - 1);
            for x in 0..len.min(w) {
                img.put_pixel(x, yy, Luma([0]));
                img.put_pixel(w - 1 - x, yy, Luma([0]));
            }
        }
    }

    // The safe-area box, drawn thick so it is unmistakable next to the rings.
    if let Some(area) = safe.and_then(|s| s.content(label)) {
        let t = 3i64;
        let (x0, y0) = (area.x as i64, area.y as i64);
        let (x1, y1) = (x0 + area.w as i64 - 1, y0 + area.h as i64 - 1);
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let inside = x >= x0 && x <= x1 && y >= y0 && y <= y1;
                let near_edge = (x - x0).abs() < t
                    || (x - x1).abs() < t
                    || (y - y0).abs() < t
                    || (y - y1).abs() < t;
                if inside && near_edge {
                    img.put_pixel(x as u32, y as u32, Luma([0]));
                }
            }
        }
    }
    img
}
