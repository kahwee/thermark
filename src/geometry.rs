//! Print geometry helpers.
//!
//! Millimetre conversion is driven by the connected printer profile. The B1
//! uses 203 dpi; newer profiles may use 300 dpi and wider or narrower heads.

/// B1 pixels per millimetre, retained for B1-specific artwork constants.
pub const PX_PER_MM: f64 = 8.0;

/// Printhead widths in pixels. Both [`crate::protocol::Model`] and
/// [`crate::print_task::PrintTask`] classify devices into one of these, so the
/// numbers live here once rather than in two parallel tables.
pub const HEAD_WIDE_PX: u32 = 384;
/// Narrow head used by the D11 / D110 family (~12 mm media).
pub const HEAD_NARROW_PX: u32 = 96;

/// Largest label dimension accepted, in mm. Nothing a pocket thermal prints
/// comes close; the bound exists so pixel math cannot overflow `u32`.
pub const MAX_DIMENSION_MM: f64 = 1000.0;

/// Convert millimetres to pixels (rounded, clamped to a sane range).
///
/// Non-finite input yields 1 px; validate with [`LabelMm::parse`] to get an
/// error instead.
pub fn mm_to_px(mm: f64, pixels_per_mm: f64) -> u32 {
    if !mm.is_finite() {
        return 1;
    }
    (mm * pixels_per_mm)
        .round()
        .clamp(1.0, MAX_DIMENSION_MM * pixels_per_mm) as u32
}

/// Convert pixels to millimetres.
pub fn px_to_mm(px: u32, pixels_per_mm: f64) -> f64 {
    px as f64 / pixels_per_mm
}

/// Label size in millimetres (across printhead × along feed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelMm {
    /// Dimension across the print head (limited by printhead width).
    pub width_mm: f64,
    /// Dimension in the paper-feed direction.
    pub height_mm: f64,
}

impl LabelMm {
    pub fn new(width_mm: f64, height_mm: f64) -> Self {
        Self {
            width_mm,
            height_mm,
        }
    }

    /// Parse `"50x30"`, `"50×30"`, or `"50*30"` (width x height, mm).
    pub fn parse(s: &str) -> crate::errors::Result<Self> {
        use crate::errors::Error;
        let s = s.trim().to_ascii_lowercase().replace(['×', '*'], "x");
        let mut parts = s.split('x');
        let w = parts
            .next()
            .and_then(|p| p.trim().parse::<f64>().ok())
            .ok_or_else(|| {
                Error::invalid_label(format!("bad label size '{s}', expected e.g. 50x30"))
            })?;
        let h = parts
            .next()
            .and_then(|p| p.trim().parse::<f64>().ok())
            .ok_or_else(|| {
                Error::invalid_label(format!("bad label size '{s}', expected e.g. 50x30"))
            })?;
        // `nan` and `inf` both parse as f64, and NaN fails every comparison —
        // so `value <= 0.0` alone would let them through into the pixel math.
        for (value, axis) in [(w, "width"), (h, "height")] {
            if !value.is_finite() || value <= 0.0 {
                return Err(Error::invalid_label(format!(
                    "label {axis} must be a positive number of mm, got '{value}'"
                )));
            }
            if value > MAX_DIMENSION_MM {
                return Err(Error::invalid_label(format!(
                    "label {axis} {value} mm exceeds the {MAX_DIMENSION_MM} mm limit"
                )));
            }
        }
        Ok(Self::new(w, h))
    }

    /// Pixel size clamped to the model printhead width.
    pub fn to_pixels(self, max_width_px: u32, pixels_per_mm: f64) -> LabelPx {
        // Rows are packed one bit per pixel, so the width must land on a byte
        // boundary. `checked_next_multiple_of` says that directly and returns
        // `None` on overflow, where `div_ceil(8) * 8` could wrap — the bug that
        // made `--label infx30` silently print an 8px-wide label.
        let w = mm_to_px(self.width_mm, pixels_per_mm)
            .checked_next_multiple_of(8)
            .unwrap_or(max_width_px)
            .min(max_width_px);

        LabelPx {
            width_px: w.max(8),
            height_px: mm_to_px(self.height_mm, pixels_per_mm).max(1),
        }
    }
}

/// An axis-aligned rectangle in label pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Per-edge inset kept clear of content.
///
/// On healthy hardware this is **registration margin, not unreachable area**:
/// a charged B1 addresses the entire canvas. It exists because labels do not
/// feed identically every time, so ink placed exactly on an edge row is
/// sometimes shaved.
///
/// Do not infer it from a single clipped print. A low battery truncates dense
/// pages part-way, which looks the same and led to a value five times too
/// large here. Measure with `thermark calibrate --boundary` on a charged
/// printer, and if two runs disagree, take the lower reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SafeArea {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

impl SafeArea {
    /// Measured default for B1-class hardware with 50x30 media.
    ///
    /// Measured with `thermark calibrate --boundary` on a **fully charged**
    /// printer: the 29 mm bar — the last one the probe can draw, covering rows
    /// 232-239 — printed complete and reached the label's edge. The printer
    /// addresses the whole canvas.
    ///
    /// Earlier readings suggested a band of 4-6 mm was unreachable. Those were
    /// taken at battery level 1: a dense page sags the supply and the printer
    /// stops mid-page, which is indistinguishable from a printable-area limit
    /// in a single sample. Charging the printer moved the "limit" by 7 mm, so
    /// it was never a limit.
    ///
    /// The 1 mm kept here is registration insurance, not unreachable area:
    /// labels do not feed identically every time, and a printed circle sitting
    /// exactly on row 0 came back with its top shaved. Set to
    /// [`SafeArea::NONE`] for true full bleed, or re-measure your own media
    /// with `thermark config safe-area --last-tick <mm>`.
    pub const B1: Self = Self {
        top: 8,
        bottom: 8,
        left: 0,
        right: 0,
    };

    /// No inset — full bleed. Use after confirming with `thermark calibrate`.
    pub const NONE: Self = Self {
        top: 0,
        bottom: 0,
        left: 0,
        right: 0,
    };

    /// Build from millimetres (what `thermark calibrate`'s ruler reads out).
    pub fn from_mm(top: f64, bottom: f64, left: f64, right: f64) -> Self {
        Self::from_mm_at(top, bottom, left, right, PX_PER_MM)
    }

    /// Build from millimetres at a printer profile's resolution.
    pub fn from_mm_at(top: f64, bottom: f64, left: f64, right: f64, pixels_per_mm: f64) -> Self {
        let px = |mm: f64| (mm.max(0.0) * pixels_per_mm).round() as u32;
        Self {
            top: px(top),
            bottom: px(bottom),
            left: px(left),
            right: px(right),
        }
    }

    /// The standard 1 mm feed-registration margin at this resolution.
    pub fn registration(pixels_per_mm: f64) -> Self {
        Self::from_mm_at(1.0, 1.0, 0.0, 0.0, pixels_per_mm)
    }

    /// The reliably printable rectangle of `label`, or `None` if the insets
    /// leave nothing.
    pub fn content(self, label: LabelPx) -> Option<Rect> {
        let horizontal = self.left.checked_add(self.right)?;
        let vertical = self.top.checked_add(self.bottom)?;
        let w = label.width_px.checked_sub(horizontal)?;
        let h = label.height_px.checked_sub(vertical)?;
        (w > 0 && h > 0).then_some(Rect {
            x: self.left,
            y: self.top,
            w,
            h,
        })
    }
}

impl Default for SafeArea {
    fn default() -> Self {
        Self::B1
    }
}

/// Label size in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelPx {
    pub width_px: u32,
    pub height_px: u32,
}

impl LabelPx {
    pub fn mm(self, pixels_per_mm: f64) -> LabelMm {
        LabelMm::new(
            px_to_mm(self.width_px, pixels_per_mm),
            px_to_mm(self.height_px, pixels_per_mm),
        )
    }
}

/// Default starter-roll size for many pocket printers (50×30 mm).
pub const DEFAULT_B1_LABEL: LabelMm = LabelMm {
    width_mm: 50.0,
    height_mm: 30.0,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifty_by_thirty() {
        let l = LabelMm::parse("50x30").unwrap();
        let p = l.to_pixels(384, 8.0);
        assert_eq!(p.width_px, 384);
        assert_eq!(p.height_px, 240);
    }

    #[test]
    fn parse_variants() {
        assert_eq!(LabelMm::parse("40×20").unwrap().height_mm, 20.0);
        assert_eq!(LabelMm::parse("30*15").unwrap().width_mm, 30.0);
    }

    #[test]
    fn width_multiple_of_eight() {
        let p = LabelMm::new(49.0, 30.0).to_pixels(384, 8.0);
        assert_eq!(p.width_px % 8, 0);
    }

    #[test]
    fn mm_px_roundtrip_approx() {
        let px = mm_to_px(30.0, 8.0);
        assert_eq!(px, 240);
        assert!((px_to_mm(240, 8.0) - 30.0).abs() < 0.01);
    }

    #[test]
    fn three_hundred_dpi_geometry_is_not_forced_to_eight_px_per_mm() {
        let profile = crate::profile::profile_for_model(crate::Model::D11H);
        let pixels =
            LabelMm::new(15.0, 30.0).to_pixels(profile.max_width_px, profile.pixels_per_mm());
        assert_eq!(pixels.width_px, 142);
        assert_eq!(pixels.height_px, 354);
    }

    #[test]
    fn registration_margin_tracks_profile_resolution() {
        assert_eq!(SafeArea::registration(8.0), SafeArea::B1);
        let area = SafeArea::registration(300.0 / 25.4);
        assert_eq!(
            (area.top, area.bottom, area.left, area.right),
            (12, 12, 0, 0)
        );
    }

    #[test]
    fn reject_bad_sizes() {
        assert!(LabelMm::parse("").is_err());
        assert!(LabelMm::parse("abc").is_err());
        assert!(LabelMm::parse("0x30").is_err());
        assert!(LabelMm::parse("-10x20").is_err());
    }

    #[test]
    fn reject_non_finite_dimensions() {
        // Both parse as f64. `inf` overflowed the multiply in `to_pixels`;
        // `nan` slipped past `<= 0.0` and produced an 8px label.
        for s in ["infx30", "50xinf", "nanx30", "50xnan", "-infx30"] {
            assert!(LabelMm::parse(s).is_err(), "should reject '{s}'");
        }
    }

    #[test]
    fn reject_absurdly_large_dimensions() {
        assert!(LabelMm::parse("1e9x30").is_err());
        assert!(LabelMm::parse("50x1e9").is_err());
        // Just inside the limit still works.
        assert!(LabelMm::parse("999x999").is_ok());
    }

    #[test]
    fn to_pixels_cannot_overflow_for_any_input() {
        // `LabelMm::new` is public and unvalidated, so the pixel math must be
        // total on its own.
        for mm in [f64::INFINITY, f64::NAN, f64::MAX, 1e300, -1.0] {
            let p = LabelMm::new(mm, mm).to_pixels(384, 8.0);
            assert!(p.width_px.is_multiple_of(8) && p.width_px <= 384);
            assert!(p.height_px >= 1);
        }
    }

    #[test]
    fn safe_area_insets_correctly() {
        let lp = LabelMm::parse("50x30").unwrap().to_pixels(384, 8.0);
        let safe = SafeArea::B1;
        // Deliberately asserts no particular asymmetry. An earlier version
        // required `bottom > top`, encoding a "the feed edge is unreachable"
        // theory that turned out to be a low battery — so the test defended
        // the wrong belief and would have resisted the correction.
        let area = safe.content(lp).unwrap();
        assert_eq!(area.x, safe.left);
        assert_eq!(area.y, safe.top);
        assert_eq!(area.w, lp.width_px - safe.left - safe.right);
        assert_eq!(area.h, lp.height_px - safe.top - safe.bottom);
        // Content never runs past the canvas.
        assert!(area.x + area.w <= lp.width_px);
        assert!(area.y + area.h <= lp.height_px);
    }

    #[test]
    fn safe_area_reports_none_when_it_would_consume_the_label() {
        let tiny = LabelPx {
            width_px: 8,
            height_px: 8,
        };
        assert!(SafeArea::B1.content(tiny).is_none());
        assert!(SafeArea::NONE.content(tiny).is_some());
    }

    #[test]
    fn safe_area_overflow_is_rejected() {
        let label = LabelPx {
            width_px: 384,
            height_px: 240,
        };
        let hostile = SafeArea {
            top: u32::MAX,
            bottom: 1,
            left: u32::MAX,
            right: 1,
        };
        assert!(hostile.content(label).is_none());
    }

    #[test]
    fn narrow_label_under_max() {
        let p = LabelMm::parse("30x20").unwrap().to_pixels(384, 8.0);
        assert_eq!(p.width_px, 240);
        assert_eq!(p.height_px, 160);
    }
}
