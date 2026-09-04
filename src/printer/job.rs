//! Image composition and high-level print-job options.

use crate::errors::Result;
use crate::geometry::{LabelMm, SafeArea};
use crate::image_encode;
use crate::types::{Density, Rotation, Threshold};
use std::path::Path;
use tracing::info;

/// Lay an image out on its label canvas exactly as printing does before
/// monochrome encoding.
///
/// Split out from the transport path so `thermark print --preview out.png` can
/// compose here, then apply the same threshold/dither pass without a printer.
pub fn compose_for_label(
    path: &Path,
    opts: &PrintOptions,
    max_width_px: u32,
    pixels_per_mm: f64,
) -> Result<image::DynamicImage> {
    let mut img = image_encode::rotate(image::open(path)?, opts.rotate);
    if opts.trim {
        img = image_encode::trim_for_print(img, opts.threshold.get(), opts.dither);
    }

    match opts.label.map(|l| l.to_pixels(max_width_px, pixels_per_mm)) {
        Some(lp) => {
            info!(
                width_px = lp.width_px,
                height_px = lp.height_px,
                width_mm = lp.mm(pixels_per_mm).width_mm,
                height_mm = lp.mm(pixels_per_mm).height_mm,
                max_w = max_width_px,
                fill = opts.fill,
                margin_px = opts.margin_px,
                dither = opts.dither,
                safe_bottom = opts.safe.bottom,
                "label canvas"
            );
            img = if opts.fill {
                image_encode::fill_label(img, lp, opts.safe, opts.margin_px)
            } else {
                image_encode::contain_label(img, lp, opts.safe, opts.margin_px)
            }?;
        }
        None if opts.fit => img = image_encode::fit_width(img, max_width_px),
        None => {}
    }
    Ok(img)
}

/// Options for a raster print job.
#[derive(Debug, Clone)]
pub struct PrintOptions {
    pub density: Density,
    pub rotate: Rotation,
    pub threshold: Threshold,
    /// Scale down only if wider than printhead.
    pub fit: bool,
    /// Physical label size (mm). Image is scaled/padded to this.
    pub label: Option<LabelMm>,
    /// If true with `label`, scale image to cover the label (may crop).
    pub fill: bool,
    /// White inset margin in pixels on each side.
    pub margin_px: u32,
    /// Floyd–Steinberg dither instead of a hard threshold.
    pub dither: bool,
    /// Content/registration insets.
    pub safe: SafeArea,
    /// Crop source whitespace before placement. Dithered images keep their
    /// full width so horizontal diffusion state remains unchanged.
    pub trim: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            density: Density::NORMAL,
            rotate: Rotation::Deg0,
            threshold: Threshold::DEFAULT,
            fit: false,
            label: None,
            fill: true,
            margin_px: 0,
            dither: false,
            safe: SafeArea::default(),
            trim: true,
        }
    }
}
