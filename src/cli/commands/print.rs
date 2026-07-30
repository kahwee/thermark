//! Raster printing: `print` and `calibrate`.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use thermark::config::Config;
use thermark::geometry::LabelMm;
use thermark::image_encode;
use thermark::printer::PrintOptions;
use thermark::protocol::Model;
use thermark::types::{Density, Rotation, Threshold};
use tracing::info;

use crate::cli::args::{ConnArgs, TaskArgs};
use crate::cli::session::print_file;
use crate::cli::tips::warn_print_limits;

#[allow(clippy::too_many_arguments)]
pub async fn print(
    cfg: &Config,
    conn: &ConnArgs,
    task: &TaskArgs,
    image: &Path,
    model: Option<Model>,
    density: Density,
    rotate: Rotation,
    threshold: Threshold,
    fit: bool,
    label: Option<String>,
    fill: bool,
    no_fill: bool,
    margin: u32,
    dither: bool,
) -> Result<()> {
    if !image.exists() {
        bail!(
            "image not found: {}\n  \
             tip: personal prints live under local/prints/ (gitignored), not fixtures/",
            image.display()
        );
    }
    warn_print_limits(image, &label, no_fill, dither);

    let model = cfg.resolve_model(model);
    let label_mm = label.as_deref().map(LabelMm::parse).transpose()?;
    // --no-fill wins over --fill; without a label there is no canvas to fill.
    let use_fill = !no_fill && label_mm.is_some() && fill;

    let opts = PrintOptions {
        density,
        rotate,
        threshold,
        fit,
        label: label_mm,
        fill: use_fill,
        margin_px: margin,
        dither,
    };
    print_file(cfg, conn, task, model, image, opts).await?;
    println!("OK — sent print job");
    Ok(())
}

pub async fn calibrate(
    cfg: &Config,
    conn: &ConnArgs,
    task: &TaskArgs,
    model: Option<Model>,
    label: &str,
    density: Density,
) -> Result<()> {
    let model = cfg.resolve_model(model);
    let label_mm = LabelMm::parse(label)?;
    let lp = label_mm.to_pixels(model.max_width_px());
    info!(
        width_px = lp.width_px,
        height_px = lp.height_px,
        width_mm = label_mm.width_mm,
        height_mm = label_mm.height_mm,
        "calibration pattern"
    );

    let tmp: PathBuf = std::env::temp_dir().join("thermark_calibrate.png");
    image_encode::calibration_pattern(lp).save(&tmp)?;

    let opts = PrintOptions {
        density,
        rotate: Rotation::Deg0,
        threshold: Threshold::DEFAULT,
        fit: false,
        label: Some(label_mm),
        fill: true,
        margin_px: 0,
        dither: false,
    };
    print_file(cfg, conn, task, model, &tmp, opts).await?;
    println!("OK — calibration printed ({label})");
    Ok(())
}
