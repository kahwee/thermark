//! Print geometry helpers.
//!
//! Community-tested resolution: **8 pixels per mm** (~203 dpi).
//! B1-class printhead max width: **384 px** (≈ 48 mm; marketing often says 50 mm).

/// Pixels per millimeter (community measurement).
pub const PX_PER_MM: f64 = 8.0;

/// Convert millimetres to pixels (rounded).
pub fn mm_to_px(mm: f64) -> u32 {
    (mm * PX_PER_MM).round().max(1.0) as u32
}

/// Convert pixels to millimetres.
pub fn px_to_mm(px: u32) -> f64 {
    px as f64 / PX_PER_MM
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
            .ok_or_else(|| Error::invalid_label(format!("bad label size '{s}', expected e.g. 50x30")))?;
        let h = parts
            .next()
            .and_then(|p| p.trim().parse::<f64>().ok())
            .ok_or_else(|| Error::invalid_label(format!("bad label size '{s}', expected e.g. 50x30")))?;
        if w <= 0.0 || h <= 0.0 {
            return Err(Error::invalid_label("label dimensions must be positive"));
        }
        Ok(Self::new(w, h))
    }

    /// Pixel size clamped to the model printhead width.
    pub fn to_pixels(self, max_width_px: u32) -> LabelPx {
        let mut w = mm_to_px(self.width_mm);
        let h = mm_to_px(self.height_mm);
        // Width must be multiple of 8 for clean byte packing
        w = w.div_ceil(8) * 8;
        if w > max_width_px {
            w = max_width_px;
        }
        LabelPx {
            width_px: w.max(8),
            height_px: h.max(1),
        }
    }
}

/// Label size in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelPx {
    pub width_px: u32,
    pub height_px: u32,
}

impl LabelPx {
    pub fn mm(self) -> LabelMm {
        LabelMm::new(px_to_mm(self.width_px), px_to_mm(self.height_px))
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
        let p = l.to_pixels(384);
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
        let p = LabelMm::new(49.0, 30.0).to_pixels(384);
        assert_eq!(p.width_px % 8, 0);
    }

    #[test]
    fn mm_px_roundtrip_approx() {
        let px = mm_to_px(30.0);
        assert_eq!(px, 240);
        assert!((px_to_mm(240) - 30.0).abs() < 0.01);
    }

    #[test]
    fn reject_bad_sizes() {
        assert!(LabelMm::parse("").is_err());
        assert!(LabelMm::parse("abc").is_err());
        assert!(LabelMm::parse("0x30").is_err());
        assert!(LabelMm::parse("-10x20").is_err());
    }

    #[test]
    fn narrow_label_under_max() {
        let p = LabelMm::parse("30x20").unwrap().to_pixels(384);
        assert_eq!(p.width_px, 240);
        assert_eq!(p.height_px, 160);
    }
}
