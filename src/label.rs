//! Compose printable labels: square QR + side text (system fonts).

use crate::errors::{Error, Result};
use crate::font::LabelFont;
use crate::geometry::{LabelPx, Rect, SafeArea};
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
    /// The reliably printable box this layout was fitted into.
    pub area: Rect,
    pub gap: u32,
    pub text_col_w: u32,
    pub qr_side: u32,
}

/// Breathing room inside the printable area, in px. Purely aesthetic — the
/// physically unprintable edges are handled by [`SafeArea`].
const MARGIN: u32 = 4;
/// Space between the QR and the text column, in px.
const GAP: u32 = 8;
/// Text column is this fraction of the label width…
const TEXT_COL_FRACTION: f64 = 0.34;
/// …but at least this many px, unless that would take over half the label.
const TEXT_COL_MIN: u32 = 64;
/// Below this, a QR holds too few modules to survive thermal printing.
const QR_SIDE_MIN: u32 = 64;

/// Compute the side-by-side layout for a label, or `None` if one does not fit.
///
/// Uses the default [`SafeArea`]; see [`qr_layout_in`] to override it.
pub fn qr_layout(label: LabelPx) -> Option<QrLayout> {
    qr_layout_in(label, SafeArea::default())
}

/// Compute the layout inside an explicit safe area.
pub fn qr_layout_in(label: LabelPx, safe: SafeArea) -> Option<QrLayout> {
    let area = safe.content(label)?;
    let inner = Rect {
        x: area.x + MARGIN,
        y: area.y + MARGIN,
        w: area.w.checked_sub(MARGIN * 2)?,
        h: area.h.checked_sub(MARGIN * 2)?,
    };

    let ideal = ((label.width_px as f64) * TEXT_COL_FRACTION).round() as u32;
    // `clamp(TEXT_COL_MIN, w / 2)` panics when `w < 2 * TEXT_COL_MIN` (every
    // D11/D110 label). The upper bound is the real constraint, so apply it last.
    let text_col_w = ideal.max(TEXT_COL_MIN).min(inner.w / 2);

    let qr_budget_w = inner.w.saturating_sub(text_col_w + GAP);
    let qr_side = qr_budget_w.min(inner.h);

    (qr_side >= QR_SIDE_MIN).then_some(QrLayout {
        area: inner,
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

    let font = load_font(opts.font_path.as_deref(), opts.font_name.as_deref())?;

    let mut img = GrayImage::from_pixel(w, h, Luma([255]));

    let QrLayout {
        area,
        gap,
        text_col_w,
        qr_side,
    } = layout;

    let (qr_x, text_x) = match opts.text_side {
        TextSide::Right => (area.x, area.x + qr_side + gap),
        TextSide::Left => (area.x + text_col_w + gap, area.x),
    };
    // Centre within the printable band, not the raw canvas — otherwise the QR
    // drifts into the clipped bottom edge.
    let qr_y = area.y + (area.h.saturating_sub(qr_side)) / 2;

    // --- Square QR ---
    let qr_img = render_qr_square(&opts.url, qr_side)?;
    overlay_gray(&mut img, &qr_img, qr_x, qr_y);

    // --- Text (system font, left-to-right, top-to-bottom) ---
    draw_text_block(
        &mut img,
        &font,
        &opts.side_text,
        Rect {
            x: text_x,
            y: area.y,
            w: text_col_w.saturating_sub(4),
            h: area.h,
        },
        TextAlign::Center,
        opts.font_size,
    );

    if opts.border {
        draw_border(&mut img);
    }

    Ok(img)
}

/// Horizontal alignment of a text block within its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum TextAlign {
    #[value(alias = "l")]
    Left,
    #[default]
    #[value(alias = "c")]
    Center,
    #[value(alias = "r")]
    Right,
}

/// Wrap, size, and draw `text` inside `bx`. Shared by every label type.
///
/// `font_size` of `None` auto-fits the largest size that keeps words whole.
pub fn draw_text_block(
    img: &mut GrayImage,
    font: &LabelFont,
    text: &str,
    bx: Rect,
    align: TextAlign,
    font_size: Option<f32>,
) {
    let px = match font_size {
        Some(s) => s.clamp(6.0, 96.0),
        None => font.fit_size(text, bx.w, bx.h),
    };
    let lines = font.wrap(text, bx.w, px);
    let line_h = font.text_height(px) as i32 + 2;
    let total_h = lines.len() as i32 * line_h;

    // Small explicit text: top-align so you can see how much fits. Otherwise
    // centre the block vertically.
    let mut baseline = if font_size.is_some() && px <= 16.0 {
        bx.y as i32 + font.text_height(px) as i32
    } else {
        bx.y as i32 + font.text_height(px) as i32 + (bx.h as i32 - total_h).max(0) / 2
    };

    for line in &lines {
        let tw = font.text_width(line, px);
        let free = bx.w.saturating_sub(tw) as f32;
        let tx = bx.x as f32
            + match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => free / 2.0,
                TextAlign::Right => free,
            };
        font.draw_text(img, tx, baseline as f32, line, px);
        baseline += line_h;
    }
}

fn draw_border(img: &mut GrayImage) {
    let (w, h) = img.dimensions();
    for x in 0..w {
        img.put_pixel(x, 0, Luma([0]));
        img.put_pixel(x, h - 1, Luma([0]));
    }
    for y in 0..h {
        img.put_pixel(0, y, Luma([0]));
        img.put_pixel(w - 1, y, Luma([0]));
    }
}

/// Options for a text-only sticker.
#[derive(Debug, Clone)]
pub struct TextLabelOptions {
    pub text: String,
    pub label: LabelPx,
    pub align: TextAlign,
    pub border: bool,
    pub font_path: Option<std::path::PathBuf>,
    pub font_name: Option<String>,
    pub font_size: Option<f32>,
}

/// Render a text-only sticker: no QR, text auto-fitted to fill the label.
pub fn make_text_label(opts: &TextLabelOptions) -> Result<GrayImage> {
    if opts.text.trim().is_empty() {
        return Err(Error::qr("text must not be empty"));
    }
    let (w, h) = (opts.label.width_px, opts.label.height_px);
    if w < 2 * MARGIN + 8 || h < 2 * MARGIN + 8 {
        return Err(Error::qr(format!(
            "label {w}x{h}px is too small to hold text with a {MARGIN}px margin"
        )));
    }

    let font = load_font(opts.font_path.as_deref(), opts.font_name.as_deref())?;
    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    draw_text_block(
        &mut img,
        &font,
        &opts.text,
        Rect {
            x: MARGIN,
            y: MARGIN,
            w: w.saturating_sub(MARGIN * 2),
            h: h.saturating_sub(MARGIN * 2),
        },
        opts.align,
        opts.font_size,
    );
    if opts.border {
        draw_border(&mut img);
    }
    Ok(img)
}

/// Load an explicit path, then a named font, then the system default.
fn load_font(path: Option<&std::path::Path>, name: Option<&str>) -> Result<LabelFont> {
    match (path, name) {
        (Some(p), _) => LabelFont::load(p),
        (None, Some(n)) => LabelFont::load_named(n),
        (None, None) => LabelFont::load_default(),
    }
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
        assert!(layout.qr_side + layout.text_col_w + layout.gap <= layout.area.w);
        assert!(layout.area.y + layout.area.h <= lp.height_px);
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
                    assert!(l.qr_side <= l.area.h);
                }
            }
        }
    }

    #[test]
    fn qr_stays_clear_of_the_clipped_bottom_edge() {
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let layout = qr_layout(lp).unwrap();
        let safe = SafeArea::default();
        // The QR must end above the band the printer cannot reach.
        let qr_bottom = layout.area.y + (layout.area.h + layout.qr_side) / 2;
        assert!(
            qr_bottom <= lp.height_px - safe.bottom,
            "QR reaches {qr_bottom}, clipped band starts at {}",
            lp.height_px - safe.bottom
        );
    }

    #[test]
    fn text_label_renders_and_stays_in_the_safe_area() {
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        let Ok(img) = make_text_label(&TextLabelOptions {
            text: "HELLO\nWORLD".into(),
            label: lp,
            align: TextAlign::Center,
            border: false,
            font_path: None,
            font_name: None,
            font_size: None,
        }) else {
            return; // no system font on this host
        };
        assert_eq!(img.dimensions(), (lp.width_px, lp.height_px));
        let safe = SafeArea::default();
        for y in (lp.height_px - safe.bottom)..lp.height_px {
            for x in 0..lp.width_px {
                assert_eq!(img.get_pixel(x, y)[0], 255, "ink at ({x},{y}) is clipped");
            }
        }
    }

    #[test]
    fn text_label_rejects_empty_text() {
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
        assert!(
            make_text_label(&TextLabelOptions {
                text: "   ".into(),
                label: lp,
                align: TextAlign::Center,
                border: false,
                font_path: None,
                font_name: None,
                font_size: None,
            })
            .is_err()
        );
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
