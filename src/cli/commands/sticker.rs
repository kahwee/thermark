//! Composed stickers: `qr` and `wifi`.
//!
//! Both render a label image, save it as PNG, then optionally print it — the
//! shared half lives in [`save_and_print`].

use anyhow::{Context, Result};
use image::GrayImage;
use std::path::PathBuf;
use thermark::config::Config;
use thermark::geometry::{LabelMm, LabelPx};
use thermark::label::{
    self, QrLabelOptions, TextAlign, TextLabelOptions, TextSide, make_text_label,
};
use thermark::printer::PrintOptions;
use thermark::protocol::Model;
use thermark::types::{Density, Rotation, Threshold};
use thermark::wifi::{WifiLabelOptions, WifiSecurity, make_wifi_label};
use tracing::info;

use crate::cli::args::{ConnArgs, FontArgs, TaskArgs};
use crate::cli::session::print_file;
use crate::cli::tips::{guard_wifi_save_path, resolve_wifi_password};

/// Resolve the model and label geometry shared by both sticker commands.
fn label_geometry(
    cfg: &Config,
    model: Option<Model>,
    label: &str,
) -> Result<(Model, LabelMm, LabelPx)> {
    let model = cfg.resolve_model(model);
    let label_mm = LabelMm::parse(label)?;
    let lp = label_mm.to_pixels(model.max_width_px());
    Ok((model, label_mm, lp))
}

/// Options a rendered sticker is printed with (no crop, no dither — line art).
fn sticker_print_options(label: LabelMm, density: Density) -> PrintOptions {
    PrintOptions {
        density,
        rotate: Rotation::Deg0,
        threshold: Threshold::DEFAULT,
        fit: false,
        label: Some(label),
        fill: false,
        margin_px: 0,
        dither: false,
        // The rendered sticker already lays out inside the printable area;
        // insetting again here would shrink it twice.
        safe: thermark::geometry::SafeArea::NONE,
    }
}

/// Save a rendered sticker, then print it unless `--no-print`.
#[allow(clippy::too_many_arguments)]
async fn save_and_print(
    cfg: &Config,
    conn: &ConnArgs,
    task: &TaskArgs,
    model: Model,
    gray: &GrayImage,
    png_path: &PathBuf,
    label_mm: LabelMm,
    density: Density,
    no_print: bool,
    success: &str,
) -> Result<()> {
    gray.save(png_path)
        .with_context(|| format!("save {}", png_path.display()))?;
    println!("saved {}", png_path.display());
    if no_print {
        return Ok(());
    }
    print_file(
        cfg,
        conn,
        task,
        model,
        png_path,
        sticker_print_options(label_mm, density),
    )
    .await?;
    println!("{success}");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn text(
    cfg: &Config,
    conn: &ConnArgs,
    task: &TaskArgs,
    font: &FontArgs,
    model: Option<Model>,
    text: &str,
    align: TextAlign,
    label: &str,
    border: bool,
    density: Density,
    save: Option<PathBuf>,
    no_print: bool,
) -> Result<()> {
    let (model, label_mm, lp) = label_geometry(cfg, model, label)?;
    let gray = make_text_label(&TextLabelOptions {
        text: text.replace("\\n", "\n"),
        label: lp,
        safe: cfg.resolve_safe_area(),
        align,
        border,
        font_path: font.font.clone(),
        font_name: font.font_name.clone(),
        font_size: font.font_size,
    })?;
    info!(
        width_px = lp.width_px,
        height_px = lp.height_px,
        ?align,
        font_size = ?font.font_size,
        "text label"
    );

    let png_path = save.unwrap_or_else(|| std::env::temp_dir().join("thermark_text_label.png"));
    save_and_print(
        cfg,
        conn,
        task,
        model,
        &gray,
        &png_path,
        label_mm,
        density,
        no_print,
        "OK — text label printed",
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn qr(
    cfg: &Config,
    conn: &ConnArgs,
    task: &TaskArgs,
    font: &FontArgs,
    model: Option<Model>,
    url: &str,
    text: &str,
    text_side: TextSide,
    label: &str,
    border: bool,
    density: Density,
    save: Option<PathBuf>,
    no_print: bool,
) -> Result<()> {
    let (model, label_mm, lp) = label_geometry(cfg, model, label)?;
    let gray = label::make_qr_label_opts(&QrLabelOptions {
        url: url.to_string(),
        side_text: text.replace("\\n", "\n"),
        label: lp,
        safe: cfg.resolve_safe_area(),
        text_side,
        border,
        font_path: font.font.clone(),
        font_name: font.font_name.clone(),
        font_size: font.font_size,
    })?;
    info!(
        width_px = lp.width_px,
        height_px = lp.height_px,
        qr_side = label::max_qr_side(lp),
        ?text_side,
        font_size = ?font.font_size,
        "qr label"
    );

    let png_path = save.unwrap_or_else(|| std::env::temp_dir().join("thermark_qr_label.png"));
    save_and_print(
        cfg,
        conn,
        task,
        model,
        &gray,
        &png_path,
        label_mm,
        density,
        no_print,
        "OK — QR label printed",
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn wifi(
    cfg: &Config,
    conn: &ConnArgs,
    task: &TaskArgs,
    font: &FontArgs,
    model: Option<Model>,
    ssid: &str,
    password: String,
    security: WifiSecurity,
    hidden: bool,
    show_password: bool,
    text_side: TextSide,
    label: &str,
    border: bool,
    density: Density,
    save: Option<PathBuf>,
    no_print: bool,
) -> Result<()> {
    let (model, label_mm, lp) = label_geometry(cfg, model, label)?;
    let password = resolve_wifi_password(password)?;
    if ssid.trim().chars().count() > 24 {
        eprintln!("tip: long SSID names wrap on a 50×30 sticker — shorter names read better");
    }
    // Never log the password (only SSID + security).
    info!(%ssid, ?security, hidden, show_password, "wifi sticker");

    let gray = make_wifi_label(&WifiLabelOptions {
        ssid: ssid.to_string(),
        password,
        security,
        hidden,
        show_password,
        label: lp,
        safe: cfg.resolve_safe_area(),
        text_side,
        font_path: font.font.clone(),
        font_name: font.font_name.clone(),
        font_size: font.font_size,
        border,
    })
    .with_context(
        || "building Wi‑Fi sticker (if QR failed: password may be too long for a dense code)",
    )?;

    let png_path = save.unwrap_or_else(|| std::env::temp_dir().join("thermark_wifi_label.png"));
    guard_wifi_save_path(&png_path)?;

    println!("SSID on sticker: {ssid}");
    if show_password {
        eprintln!(
            "warning: password is printed in cleartext on the label \
             (anyone who sees the sticker can read it)"
        );
    } else {
        println!("password: in QR only (not printed as text)");
    }

    save_and_print(
        cfg,
        conn,
        task,
        model,
        &gray,
        &png_path,
        label_mm,
        density,
        no_print,
        "OK — Wi‑Fi sticker printed",
    )
    .await
}
