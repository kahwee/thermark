//! Raster printing: `print` and `calibrate`.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use thermark::config::Config;
use thermark::geometry::LabelMm;
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
    no_trim: bool,
    full_bleed: bool,
    preview: Option<PathBuf>,
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
        safe: if full_bleed {
            thermark::geometry::SafeArea::NONE
        } else {
            cfg.resolve_safe_area()
        },
        trim: !no_trim,
    };
    if let Some(out) = preview {
        // Compose through the exact same path a real print uses, then stop.
        let max_w = thermark::print_task::effective_max_width_px(
            model,
            crate::cli::session::resolve_task(model, task)?,
        );
        let composed = thermark::printer::compose_for_label(image, &opts, max_w)?;
        composed
            .to_luma8()
            .save(&out)
            .with_context(|| format!("save {}", out.display()))?;
        println!("preview written to {} (nothing printed)", out.display());
        return Ok(());
    }

    print_file(cfg, conn, task, model, image, opts).await?;
    println!("OK — sent print job");
    Ok(())
}

/// Explain how to read the pattern, so a re-run needs no other reference.
fn print_calibration_legend(lp: thermark::geometry::LabelPx, safe: thermark::geometry::SafeArea) {
    use thermark::image_encode::{CALIBRATION_RING_STEP_PX, CALIBRATION_RINGS};

    let step_mm = CALIBRATION_RING_STEP_PX as f64 / thermark::geometry::PX_PER_MM;
    println!();
    println!("How to read it:");
    println!(
        "  THICK box  = the configured safe area (top {} / bottom {} / left {} / right {} px).",
        safe.top, safe.bottom, safe.left, safe.right
    );
    println!("               If it printed complete on all 4 sides, you are good.");
    println!(
        "  THIN rings = {CALIBRATION_RINGS} rings, {step_mm} mm apart, outermost at the very edge."
    );
    println!("               Count how many are complete on each edge:");
    println!("               ring N complete  ->  that edge needs {step_mm} mm x N of margin.");
    println!("  SIDE ticks = a feed ruler from the top edge: short = 1 mm,");
    println!("               long = 5 mm. Read off the LAST tick that printed —");
    println!("               that is your usable height; the rest is lost at the");
    println!("               feed edge. e.g. last long tick at 25 mm on a 30 mm");
    println!("               label  ->  bottom inset needs ~5 mm (40 px).");
    println!("  Diagonals  = skew check; the X should meet exactly at the centre cross.");
    println!();
    println!("Some white border is physical, not a bug:");
    println!("  Across: the printhead is 48 mm; a 50 mm label keeps ~2 mm.");
    println!("          If it is lopsided, re-centre the roll with the guide.");
    println!("  Feed:   the printer starts a little after the label's leading");
    println!("          edge and stops before its trailing edge (~2 mm each on");
    println!("          B1 + 50x30). No setting fills those; the inset below");
    println!("          just stops content being silently dropped there.");
    println!();
    println!(
        "Canvas {}x{} px. Re-run any time:",
        lp.width_px, lp.height_px
    );
    println!("  thermark calibrate --label 50x30");
}

/// How to read the boundary probe.
fn print_boundary_legend(label: thermark::geometry::LabelPx) {
    let range = thermark::label::boundary_range(label);
    let (from, to) = (range.start(), range.end());
    println!();
    println!("One numbered bar per millimetre, {from}..{to} mm from the top.");
    println!("Each sits at its own horizontal position, so nothing crowds.");
    println!();
    println!("  Read the HIGHEST number whose bar printed completely.");
    println!("  That is where this printer stops on this media.");
    println!();
    println!("  If the LAST bar printed, the printer reaches the whole label —");
    println!("  there is no unprintable band, and any inset is only");
    println!("  registration margin. Charge first: a low battery truncates");
    println!("  dense pages and reads as a printable-area limit.");
    println!();
    println!("Then save it — the tool works out the inset:");
    println!("  thermark config safe-area --last-tick <that number> --label 50x30");
    println!();
    println!("Registration varies slightly between labels, so if two runs differ,");
    println!("use the LOWER number.");
}

pub async fn calibrate(
    cfg: &Config,
    conn: &ConnArgs,
    task: &TaskArgs,
    model: Option<Model>,
    label: Option<&str>,
    density: Density,
    boundary: bool,
) -> Result<()> {
    let model = cfg.resolve_model(model);
    let label = cfg.resolve_label(label);
    let label_mm = LabelMm::parse(&label)?;
    let lp = label_mm.to_pixels(model.max_width_px());
    info!(
        width_px = lp.width_px,
        height_px = lp.height_px,
        width_mm = label_mm.width_mm,
        height_mm = label_mm.height_mm,
        "calibration pattern"
    );

    let tmp: PathBuf = std::env::temp_dir().join("thermark_calibrate.png");
    if boundary {
        thermark::label::make_boundary_label(lp)?.save(&tmp)?;
    } else {
        thermark::label::make_calibration_label(lp, cfg.resolve_safe_area())?.save(&tmp)?;
    }

    let opts = PrintOptions {
        density,
        rotate: Rotation::Deg0,
        threshold: Threshold::DEFAULT,
        fit: false,
        label: Some(label_mm),
        fill: true,
        margin_px: 0,
        dither: false,
        // Full bleed on purpose: this pattern exists to find the true edges,
        // so it must not be inset by the value it is measuring, nor trimmed.
        safe: thermark::geometry::SafeArea::NONE,
        trim: false,
    };
    print_file(cfg, conn, task, model, &tmp, opts).await?;
    if boundary {
        println!("OK — boundary probe printed ({label})");
        print_boundary_legend(lp);
        return Ok(());
    }
    println!("OK — calibration printed ({label})");
    print_calibration_legend(lp, cfg.resolve_safe_area());
    Ok(())
}
