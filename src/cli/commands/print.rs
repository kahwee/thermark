//! Raster printing: `print` and `calibrate`.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use thermark::config::Config;
use thermark::geometry::LabelMm;
use thermark::printer::PrintOptions;
use thermark::protocol::Model;
use thermark::types::{Density, Rotation, Threshold};
use tracing::info;

use crate::cli::args::{ConnArgs, TaskArgs};
use crate::cli::session::{print_file_resolved, print_gray_resolved, resolve_profile};
use crate::cli::tips::warn_print_limits;

pub struct PrintCommand {
    pub conn: ConnArgs,
    pub task: TaskArgs,
    pub image: PathBuf,
    pub model: Option<Model>,
    pub density: Density,
    pub rotate: Rotation,
    pub threshold: Threshold,
    pub fit: bool,
    pub label: Option<String>,
    pub fill: bool,
    pub no_fill: bool,
    pub margin: u32,
    pub dither: bool,
    pub no_trim: bool,
    pub full_bleed: bool,
    pub preview: Option<PathBuf>,
}

pub async fn print(cfg: &Config, args: PrintCommand) -> Result<()> {
    let PrintCommand {
        conn,
        task,
        image,
        model,
        density,
        rotate,
        threshold,
        fit,
        label,
        fill,
        no_fill,
        margin,
        dither,
        no_trim,
        full_bleed,
        preview,
    } = args;
    if !image.exists() {
        bail!(
            "image not found: {}\n  \
             tip: personal prints live under local/prints/ (gitignored), not fixtures/",
            image.display()
        );
    }
    warn_print_limits(&image, &label, no_fill, dither);

    let profile = resolve_profile(cfg, model, &task)?;
    let model = profile.model;
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
        let composed = thermark::printer::compose_for_label(&image, &opts, profile.max_width_px)?;
        composed
            .to_luma8()
            .save(&out)
            .with_context(|| format!("save {}", out.display()))?;
        println!("preview written to {} (nothing printed)", out.display());
        return Ok(());
    }

    print_file_resolved(cfg, &conn, model, profile.task, &image, opts).await?;
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
    println!("               that is the observed reach for this charged run.");
    println!("               If repeated runs disagree, charge the printer before");
    println!("               treating the result as a registration margin.");
    println!("  Diagonals  = skew check; the X should meet exactly at the centre cross.");
    println!();
    println!("Some side-to-side white border is physical, not a bug:");
    println!("  Across: the printhead is 48 mm; a 50 mm label keeps ~2 mm.");
    println!("          If it is lopsided, re-centre the roll with the guide.");
    println!("  Feed:   a charged B1 reaches the whole canvas. The default 1 mm");
    println!("          top/bottom inset is registration insurance only.");
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

pub struct CalibrateCommand {
    pub conn: ConnArgs,
    pub task: TaskArgs,
    pub model: Option<Model>,
    pub label: Option<String>,
    pub density: Density,
    pub boundary: bool,
}

pub async fn calibrate(cfg: &Config, args: CalibrateCommand) -> Result<()> {
    let CalibrateCommand {
        conn,
        task,
        model,
        label,
        density,
        boundary,
    } = args;
    let profile = resolve_profile(cfg, model, &task)?;
    let label = cfg.resolve_label(label.as_deref());
    let label_mm = LabelMm::parse(&label)?;
    let lp = label_mm.to_pixels(profile.max_width_px);
    info!(
        width_px = lp.width_px,
        height_px = lp.height_px,
        width_mm = label_mm.width_mm,
        height_mm = label_mm.height_mm,
        "calibration pattern"
    );

    let gray = if boundary {
        thermark::label::make_boundary_label(lp, None)?
    } else {
        thermark::label::make_calibration_label(lp, cfg.resolve_safe_area(), None)?
    };
    print_gray_resolved(cfg, &conn, profile, &gray, density).await?;
    if boundary {
        println!("OK — boundary probe printed ({label})");
        print_boundary_legend(lp);
        return Ok(());
    }
    println!("OK — calibration printed ({label})");
    print_calibration_legend(lp, cfg.resolve_safe_area());
    Ok(())
}
