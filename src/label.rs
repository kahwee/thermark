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
    /// Content/registration insets for this printer/media. `SafeArea::default()` is the
    /// measured B1 value; override from `Config::resolve_safe_area`.
    pub safe: SafeArea,
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

/// Breathing room inside the content area, in px. Purely aesthetic.
const MARGIN: u32 = 4;
/// Space between the QR and the text column, in px.
const GAP: u32 = 8;
/// Text column is this fraction of the label width…
const TEXT_COL_FRACTION: f64 = 0.34;
/// …but at least this many px, unless that would take over half the label.
const TEXT_COL_MIN: u32 = 64;
/// Below this, a QR holds too few modules to survive thermal printing.
const QR_SIDE_MIN: u32 = 64;
/// Smallest box worth laying text into, on either axis.
const MIN_TEXT_BOX_PX: u32 = 16;

/// The box every label type composes into: the label area, inset by the
/// cosmetic `MARGIN`.
///
/// **One owner for this calculation.** It was previously computed separately in
/// `qr_layout` and `make_text_label`, and the two drifted: the text path was
/// left laying out on the raw label, giving a box 232 rows tall on 50x30 media
/// instead of 184, so text was sized for space the printer cannot reach.
pub fn content_box(label: LabelPx, safe: SafeArea) -> Option<Rect> {
    let area = safe.content(label)?;
    Some(Rect {
        x: area.x + MARGIN,
        y: area.y + MARGIN,
        w: area.w.checked_sub(MARGIN * 2)?,
        h: area.h.checked_sub(MARGIN * 2)?,
    })
}

/// Compute the side-by-side layout inside the content area of `label`.
pub fn qr_layout(label: LabelPx, safe: SafeArea) -> Option<QrLayout> {
    let inner = content_box(label, safe)?;

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
    let layout = qr_layout(opts.label, opts.safe).ok_or_else(|| {
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
    let line_h = font.text_height(px) as f32 + 2.0;
    let top_align = font_size.is_some() && px <= 16.0;

    // Place by measured ink, not by font metrics — see `block_ink_bounds`.
    let mut baseline = match font.block_ink_bounds(&lines, px, line_h) {
        Some((ink_top, ink_bottom)) => {
            let ink_h = ink_bottom - ink_top;
            let offset = if top_align {
                0.0
            } else {
                ((bx.h as f32 - ink_h) / 2.0).max(0.0)
            };
            bx.y as f32 + offset - ink_top
        }
        // Nothing to measure (blank text): fall back to metrics.
        None => bx.y as f32 + font.ascent(px) as f32,
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
        font.draw_text(img, tx, baseline, line, px);
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

/// Calibration pattern with printed millimetre numbers down the feed ruler.
///
/// The geometry comes from [`crate::image_encode::calibration_pattern`];
/// this adds the numerals, which need font access. Numbers make the photo
/// self-describing: read the last one that printed rather than counting ticks
/// from an edge that may itself be cut off.
pub fn make_calibration_label(
    label: LabelPx,
    safe: SafeArea,
    pixels_per_mm: f64,
    font_path: Option<&std::path::Path>,
) -> Result<GrayImage> {
    let mut img = crate::image_encode::calibration_pattern(label, Some(safe), pixels_per_mm);
    let font = match load_font(font_path, None) {
        Ok(f) => f,
        // No system font: the geometric pattern is still perfectly usable.
        Err(_) => return Ok(img),
    };

    let scale = pixels_per_mm / crate::geometry::PX_PER_MM;
    let size = (13.0 * scale) as f32;
    // Clear of the ruler ticks, derived from their length rather than a second
    // constant that has to be kept in agreement by hand.
    let inset =
        ((crate::image_encode::CALIBRATION_RULER_MAJOR_PX + 4) as f64 * scale).round() as u32;

    let height_mm = (f64::from(label.height_px) / pixels_per_mm).floor() as u32;
    for mm in (0..=height_mm).step_by(5) {
        let y = (f64::from(mm) * pixels_per_mm).round() as u32;
        if y >= label.height_px {
            break;
        }
        let text = mm.to_string();
        let w = font.text_width(&text, size);
        if inset + w >= label.width_px - inset {
            break; // too narrow to letter without colliding
        }
        // Baseline sits just ABOVE its tick. Below the tick, the last numeral
        // falls past the printable band — and that is the reading that matters
        // most, so it is exactly the one you must not lose.
        let baseline = y.max(size as u32) as f32 - 1.0;
        font.draw_text(&mut img, inset as f32, baseline, &text, size);
        font.draw_text(
            &mut img,
            (label.width_px - inset - w) as f32,
            baseline,
            &text,
            size,
        );
    }
    Ok(img)
}

/// How many millimetres the boundary probe covers, ending at the label's edge.
///
/// Thirteen bars leaves each numeral room on the narrowest supported probe;
/// the window is placed at the *end* of the label because that is the edge in
/// question.
pub const BOUNDARY_SPAN_MM: u32 = 13;

/// Millimetre range the boundary probe marks on `label`: the last
/// [`BOUNDARY_SPAN_MM`] millimetres, ending at the final drawable one.
///
/// Derived from the label rather than fixed. A hardcoded 17..29 drew three
/// bars on 40x20 media and probed the middle of a 50x80 label — nowhere near
/// the edge it exists to measure.
pub fn boundary_range(label: LabelPx, pixels_per_mm: f64) -> std::ops::RangeInclusive<u32> {
    let last = (f64::from(label.height_px) / pixels_per_mm)
        .floor()
        .max(1.0) as u32
        - 1;
    let first = last.saturating_sub(BOUNDARY_SPAN_MM - 1);
    first..=last
}

/// A staircase of numbered bars, one per millimetre, for finding exactly where
/// the printer stops.
///
/// The feed ruler on [`make_calibration_label`] puts its ticks 1 mm (8 px)
/// apart, which is too crowded to letter every one — so readings there are
/// "somewhere past 20", and estimating the rest from a photo is what produced
/// two contradictory measurements. Here each millimetre gets its own bar at its
/// own horizontal position, so the numbers never crowd: **the last bar you can
/// see is the answer**, with no counting and no scale estimation.
pub fn make_boundary_label(
    label: LabelPx,
    pixels_per_mm: f64,
    font_path: Option<&std::path::Path>,
) -> Result<GrayImage> {
    let (w, h) = (label.width_px, label.height_px);
    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    let font = load_font(font_path, None)?;

    let range = boundary_range(label, pixels_per_mm);
    let steps = range.end() - range.start() + 1;
    let slot = w / steps.max(1);
    let bar_w = slot.saturating_sub(4).max(1);
    let size = 12.0f32;

    for (i, mm) in range.enumerate() {
        let i = i as u32;
        let y = (f64::from(mm) * pixels_per_mm).round() as u32;
        let next_y = (f64::from(mm + 1) * pixels_per_mm).round() as u32;
        if next_y > h {
            break;
        }
        let x0 = i * slot;

        // Bar: 1 mm tall, so its own thickness cannot be mistaken for a
        // neighbour's.
        for yy in y..next_y.min(h) {
            for xx in x0..(x0 + bar_w).min(w) {
                img.put_pixel(xx, yy, Luma([0]));
            }
        }
        // Number sits above its bar. Horizontal separation is what keeps these
        // legible where a shared ruler column could not.
        let baseline = y as f32 - 2.0;
        if baseline > size {
            font.draw_text(&mut img, x0 as f32, baseline, &mm.to_string(), size);
        }
    }
    Ok(img)
}

/// Options for a text-only sticker.
#[derive(Debug, Clone)]
pub struct TextLabelOptions {
    pub text: String,
    pub label: LabelPx,
    pub safe: SafeArea,
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
    let area = content_box(opts.label, opts.safe)
        .filter(|a| a.w >= MIN_TEXT_BOX_PX && a.h >= MIN_TEXT_BOX_PX)
        .ok_or_else(|| {
            Error::qr(format!(
                "label {w}x{h}px leaves no printable room for text after insets"
            ))
        })?;

    let font = load_font(opts.font_path.as_deref(), opts.font_name.as_deref())?;
    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    draw_text_block(
        &mut img,
        &font,
        &opts.text,
        area,
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
        safe: SafeArea::default(),
        text_side,
        border: false,
        font_path: None,
        font_name: None,
        font_size: None,
    })
}

/// Smallest module size a thermal print can hold and still scan reliably.
///
/// Below this the code is emitted but unreadable — heat bleed closes the gaps
/// between modules. Failing loudly beats handing someone a sticker that looks
/// right and never scans.
pub const QR_MIN_MODULE_PX: usize = 2;
/// Below this it scans, but marginally; worth saying so.
pub const QR_COMFORTABLE_MODULE_PX: usize = 3;

pub fn render_qr_square(url: &str, side: u32) -> Result<GrayImage> {
    let code = QrCode::new(url.as_bytes()).map_err(|e| Error::qr(format!("QR encode: {e}")))?;
    let colors = code.to_colors();
    let modules = code.width();
    let quiet = 2usize;
    let total = modules + quiet * 2;
    let mpx = side as usize / total;

    if mpx < QR_MIN_MODULE_PX {
        return Err(Error::qr(format!(
            "content needs {modules} QR modules, which is {mpx}px per module in {side}px — \
             too fine to scan once printed (need {QR_MIN_MODULE_PX}px). \
             Shorten the content, or use a larger label."
        )));
    }
    if mpx < QR_COMFORTABLE_MODULE_PX {
        tracing::warn!(
            module_px = mpx,
            modules,
            "QR modules are small; it should scan but shorten the content if it does not"
        );
    }
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
///
/// Takes the same `safe` the renderer will: reporting against the default
/// while the caller renders with a configured one prints a number that does
/// not match the label.
pub fn max_qr_side(label: LabelPx, safe: SafeArea) -> u32 {
    qr_layout(label, safe).map_or(0, |l| l.qr_side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::LabelMm;

    fn test_font_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf")
    }

    #[test]
    fn qr_is_square() {
        let side = 200;
        let qr = render_qr_square("https://www.youtube.com", side).unwrap();
        assert_eq!(qr.dimensions(), (side, side));
    }

    #[test]
    fn label_text_is_left_to_right_abc() {
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
        let img = make_qr_label_opts(&QrLabelOptions {
            url: "https://www.youtube.com".into(),
            side_text: "ABC".into(),
            label: lp,
            safe: SafeArea::default(),
            text_side: TextSide::Right,
            border: false,
            font_path: Some(test_font_path()),
            font_name: None,
            font_size: None,
        })
        .unwrap();
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
        assert_eq!(max_qr_side(lp, SafeArea::default()), 0);
        let err = make_qr_label("https://example.com", "HI", lp, TextSide::Right)
            .expect_err("96px label cannot fit QR + text");
        assert!(err.to_string().contains("too small"), "{err}");
    }

    #[test]
    fn qr_layout_is_the_only_source_of_layout_math() {
        // `max_qr_side` must report exactly what the renderer uses, or the two
        // copies of this math drift.
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
        let layout = qr_layout(lp, SafeArea::default()).expect("50x30 fits");
        assert_eq!(max_qr_side(lp, SafeArea::default()), layout.qr_side);
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
                if let Some(l) = qr_layout(lp, SafeArea::default()) {
                    assert!(l.qr_side >= QR_SIDE_MIN);
                    assert!(l.qr_side <= l.area.h);
                }
            }
        }
    }

    #[test]
    fn qr_rejects_content_too_dense_to_scan() {
        // Long content used to render 1px modules into a 200px square: it
        // looked like a QR and was unscannable. 900 bytes needs 113 modules,
        // which is 1px each here.
        let long = "x".repeat(900);
        let err = render_qr_square(&long, 200).expect_err("should refuse");
        assert!(err.to_string().contains("too fine to scan"), "{err}");

        // The boundary: 600 bytes is 93 modules -> 2px, which still scans.
        assert!(render_qr_square(&"x".repeat(600), 200).is_ok());
    }

    #[test]
    fn qr_accepts_ordinary_content() {
        let img = render_qr_square("https://github.com/kahwee/thermark", 200).unwrap();
        assert_eq!(img.dimensions(), (200, 200));
    }

    #[test]
    fn qr_stays_clear_of_the_clipped_bottom_edge() {
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
        let layout = qr_layout(lp, SafeArea::default()).unwrap();
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
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
        let img = make_text_label(&TextLabelOptions {
            text: "HELLO\nWORLD".into(),
            label: lp,
            safe: SafeArea::default(),
            align: TextAlign::Center,
            border: false,
            font_path: Some(test_font_path()),
            font_name: None,
            font_size: None,
        })
        .expect("render text label with vendored font");
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
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
        assert!(
            make_text_label(&TextLabelOptions {
                text: "   ".into(),
                label: lp,
                safe: SafeArea::default(),
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
        let img = make_qr_label_opts(&QrLabelOptions {
            url: "https://example.com".into(),
            side_text: "HI".into(),
            label: lp,
            safe: SafeArea::default(),
            text_side: TextSide::Right,
            border: false,
            font_path: Some(test_font_path()),
            font_name: None,
            font_size: None,
        })
        .unwrap();
        // Corners should be white (no border)
        assert_eq!(img.get_pixel(0, 0)[0], 255);
        assert_eq!(img.get_pixel(383, 0)[0], 255);
        assert_eq!(img.get_pixel(0, 239)[0], 255);
        assert_eq!(img.get_pixel(383, 239)[0], 255);
    }
}
