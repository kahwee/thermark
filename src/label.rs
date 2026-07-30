//! Compose printable labels: square QR + side text (system fonts).

use crate::errors::{Error, Result};
use crate::font::LabelFont;
use crate::geometry::LabelPx;
use image::{GrayImage, Luma};
use qrcode::QrCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum TextSide {
    /// Text column on the left of the QR.
    #[value(alias = "l")]
    Left,
    /// Text column on the right of the QR.
    #[value(alias = "r")]
    Right,
}

#[derive(Debug, Clone)]
pub struct QrLabelOptions {
    pub url: String,
    pub side_text: String,
    pub label: LabelPx,
    pub text_side: TextSide,
    /// Draw a 1px outer border (off by default — not needed for printing).
    pub border: bool,
    /// Optional explicit font path; otherwise auto-pick system Arial/etc.
    pub font_path: Option<std::path::PathBuf>,
    /// Named font: "helvetica", "times", "arial", …
    pub font_name: Option<String>,
    /// Fixed text size in pixels (approx em height). `None` = auto-fit largest.
    pub font_size: Option<f32>,
}

/// Placement of the QR and text column on a label canvas.
///
/// Single source of truth for the side-by-side geometry — [`max_qr_side`] and
/// [`make_qr_label_opts`] both read it so they cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QrLayout {
    pub margin: u32,
    pub gap: u32,
    pub text_col_w: u32,
    pub qr_side: u32,
}

/// White space around the label edge, in px.
const MARGIN: u32 = 6;
/// Space between the QR and the text column, in px.
const GAP: u32 = 8;
/// Text column is this fraction of the label width…
const TEXT_COL_FRACTION: f64 = 0.34;
/// …but at least this many px, unless that would take over half the label.
const TEXT_COL_MIN: u32 = 64;
/// Below this, a QR holds too few modules to survive thermal printing.
const QR_SIDE_MIN: u32 = 64;

/// Compute the side-by-side layout for a label, or `None` if one does not fit.
pub fn qr_layout(label: LabelPx) -> Option<QrLayout> {
    let w = label.width_px;
    let h = label.height_px;

    let ideal = ((w as f64) * TEXT_COL_FRACTION).round() as u32;
    // `clamp(TEXT_COL_MIN, w / 2)` panics when `w < 2 * TEXT_COL_MIN` (every
    // D11/D110 label). The upper bound is the real constraint, so apply it last.
    let text_col_w = ideal.max(TEXT_COL_MIN).min(w / 2);

    let qr_budget_w = w.saturating_sub(text_col_w + MARGIN * 2 + GAP);
    let qr_budget_h = h.saturating_sub(MARGIN * 2);
    let qr_side = qr_budget_w.min(qr_budget_h);

    (qr_side >= QR_SIDE_MIN).then_some(QrLayout {
        margin: MARGIN,
        gap: GAP,
        text_col_w,
        qr_side,
    })
}

/// Build a label: **square** QR + readable side text using a system TTF font.
pub fn make_qr_label_opts(opts: &QrLabelOptions) -> Result<GrayImage> {
    if opts.url.is_empty() {
        return Err(Error::qr("url must not be empty"));
    }
    let w = opts.label.width_px;
    let h = opts.label.height_px;
    let layout = qr_layout(opts.label).ok_or_else(|| {
        Error::qr(format!(
            "label {w}x{h}px is too small for a QR beside text \
             (need at least {QR_SIDE_MIN}px of QR after margins). \
             Use a wider label, or a model with a wider printhead."
        ))
    })?;

    let font = if let Some(ref p) = opts.font_path {
        LabelFont::load(p)?
    } else if let Some(ref n) = opts.font_name {
        LabelFont::load_named(n)?
    } else {
        LabelFont::load_default()?
    };

    let mut img = GrayImage::from_pixel(w, h, Luma([255]));

    let QrLayout {
        margin,
        gap,
        text_col_w,
        qr_side,
    } = layout;

    let (qr_x, text_x) = match opts.text_side {
        TextSide::Right => (margin, margin + qr_side + gap),
        TextSide::Left => (margin + text_col_w + gap, margin),
    };
    let qr_y = (h.saturating_sub(qr_side)) / 2;

    // --- Square QR ---
    let qr_img = render_qr_square(&opts.url, qr_side)?;
    overlay_gray(&mut img, &qr_img, qr_x, qr_y);

    // --- Text (system font, left-to-right, top-to-bottom) ---
    let max_w = text_col_w.saturating_sub(4);
    let max_h = h.saturating_sub(margin * 2);
    // Auto-fit unless caller requested an explicit (e.g. small) size.
    let px = match opts.font_size {
        Some(s) => s.clamp(6.0, 96.0),
        None => font.fit_size(&opts.side_text, max_w, max_h),
    };
    let lines = font.wrap(&opts.side_text, max_w, px);
    let line_h = font.text_height(px) as i32 + 2;
    let total_h = lines.len() as i32 * line_h;
    // Small text: top-align so you can see how much fits; large: center.
    let mut baseline = if opts.font_size.is_some() && px <= 16.0 {
        margin as i32 + font.text_height(px) as i32
    } else {
        margin as i32 + font.text_height(px) as i32 + (max_h as i32 - total_h).max(0) / 2
    };

    for line in &lines {
        let tw = font.text_width(line, px);
        let tx = text_x as f32 + (max_w.saturating_sub(tw) as f32 / 2.0).max(0.0);
        font.draw_text(&mut img, tx, baseline as f32, line, px);
        baseline += line_h;
    }

    if opts.border {
        for x in 0..w {
            img.put_pixel(x, 0, Luma([0]));
            img.put_pixel(x, h - 1, Luma([0]));
        }
        for y in 0..h {
            img.put_pixel(0, y, Luma([0]));
            img.put_pixel(w - 1, y, Luma([0]));
        }
    }

    Ok(img)
}

/// Convenience wrapper.
pub fn make_qr_label(
    url: &str,
    side_text: &str,
    label: LabelPx,
    text_side: TextSide,
) -> Result<GrayImage> {
    make_qr_label_opts(&QrLabelOptions {
        url: url.into(),
        side_text: side_text.into(),
        label,
        text_side,
        border: false,
        font_path: None,
        font_name: None,
        font_size: None,
    })
}

pub fn render_qr_square(url: &str, side: u32) -> Result<GrayImage> {
    let code = QrCode::new(url.as_bytes()).map_err(|e| Error::qr(format!("QR encode: {e}")))?;
    let colors = code.to_colors();
    let modules = code.width();
    let quiet = 2usize;
    let total = modules + quiet * 2;
    let mpx = (side as usize / total).max(1);
    let drawn = (total * mpx) as u32;
    let ox = (side.saturating_sub(drawn)) / 2;
    let oy = ox;

    let mut img = GrayImage::from_pixel(side, side, Luma([255]));
    for y in 0..modules {
        for x in 0..modules {
            if colors[y * modules + x] != qrcode::Color::Dark {
                continue;
            }
            let px = ox + ((x + quiet) * mpx) as u32;
            let py = oy + ((y + quiet) * mpx) as u32;
            for dy in 0..mpx as u32 {
                for dx in 0..mpx as u32 {
                    if px + dx < side && py + dy < side {
                        img.put_pixel(px + dx, py + dy, Luma([0]));
                    }
                }
            }
        }
    }
    Ok(img)
}

fn overlay_gray(dst: &mut GrayImage, src: &GrayImage, x0: u32, y0: u32) {
    let (dw, dh) = dst.dimensions();
    let (sw, sh) = src.dimensions();
    for y in 0..sh {
        for x in 0..sw {
            let dx = x0 + x;
            let dy = y0 + y;
            if dx < dw && dy < dh {
                dst.put_pixel(dx, dy, *src.get_pixel(x, y));
            }
        }
    }
}

/// QR edge length [`make_qr_label_opts`] will use, or 0 when none fits.
pub fn max_qr_side(label: LabelPx) -> u32 {
    qr_layout(label).map_or(0, |l| l.qr_side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::LabelMm;

    #[test]
    fn qr_is_square() {
        let side = 200;
        let qr = render_qr_square("https://www.youtube.com", side).unwrap();
        assert_eq!(qr.dimensions(), (side, side));
    }

    #[test]
    fn label_text_is_left_to_right_abc() {
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let img = make_qr_label("https://www.youtube.com", "ABC", lp, TextSide::Right).unwrap();
        assert_eq!(img.dimensions(), (384, 240));

        // Text lives in the right half; find dark runs and ensure width is reasonable
        let mut min_x = 9999u32;
        let mut max_x = 0u32;
        let mut count = 0u32;
        for (x, y, p) in img.enumerate_pixels() {
            if x > 200 && p[0] < 128 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                count += 1;
                let _ = y;
            }
        }
        assert!(count > 50, "expected text ink, got {count}");
        assert!(max_x > min_x + 15, "ABC should span horizontally");
    }

    #[test]
    fn narrow_label_errors_instead_of_panicking() {
        // D11/D110 printheads are 96px, so `text_col_w.clamp(64, w / 2)` was
        // clamp(64, 48) — an unconditional panic for every QR on those models.
        let lp = LabelPx {
            width_px: 96,
            height_px: 240,
        };
        assert_eq!(max_qr_side(lp), 0);
        let err = make_qr_label("https://example.com", "HI", lp, TextSide::Right)
            .expect_err("96px label cannot fit QR + text");
        assert!(err.to_string().contains("too small"), "{err}");
    }

    #[test]
    fn qr_layout_is_the_only_source_of_layout_math() {
        // `max_qr_side` must report exactly what the renderer uses, or the two
        // copies of this math drift.
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let layout = qr_layout(lp).expect("50x30 fits");
        assert_eq!(max_qr_side(lp), layout.qr_side);
        assert!(layout.text_col_w <= lp.width_px / 2);
        assert!(layout.qr_side + layout.text_col_w + layout.margin * 2 + layout.gap <= lp.width_px);
    }

    #[test]
    fn qr_layout_never_panics_across_label_sizes() {
        for w in (8..=384).step_by(8) {
            for h in [8u32, 32, 96, 240, 800] {
                let lp = LabelPx {
                    width_px: w,
                    height_px: h,
                };
                if let Some(l) = qr_layout(lp) {
                    assert!(l.qr_side >= QR_SIDE_MIN);
                    assert!(l.qr_side <= h.saturating_sub(l.margin * 2));
                }
            }
        }
    }

    #[test]
    fn no_border_by_default() {
        let lp = LabelPx {
            width_px: 384,
            height_px: 240,
        };
        let img = make_qr_label("https://example.com", "HI", lp, TextSide::Right).unwrap();
        // Corners should be white (no border)
        assert_eq!(img.get_pixel(0, 0)[0], 255);
        assert_eq!(img.get_pixel(383, 0)[0], 255);
        assert_eq!(img.get_pixel(0, 239)[0], 255);
        assert_eq!(img.get_pixel(383, 239)[0], 255);
    }
}
