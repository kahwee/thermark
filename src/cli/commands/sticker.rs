//! Composed stickers: `qr` and `wifi`.
//!
//! Each command renders in memory, optionally saves a PNG, then optionally
//! prints through the shared output path.

use anyhow::{Context, Result};
use image::GrayImage;
use std::path::PathBuf;
use thermark::config::Config;
use thermark::geometry::{LabelMm, LabelPx};
use thermark::label::{
    self, QrLabelOptions, TextAlign, TextLabelOptions, TextSide, make_text_label,
};
use thermark::protocol::Model;
use thermark::types::Density;
use thermark::wifi::{WifiLabelOptions, WifiSecurity, make_wifi_label};
use tracing::info;

use crate::cli::args::{ConnArgs, FontArgs, TaskArgs};
use crate::cli::session::{PrintProfile, print_gray_resolved, resolve_profile};
use crate::cli::tips::{guard_wifi_save_path, resolve_wifi_password};

/// Resolve the model and label geometry shared by both sticker commands.
fn label_geometry(
    cfg: &Config,
    task: &TaskArgs,
    model: Option<Model>,
    label: Option<&str>,
) -> Result<(PrintProfile, LabelMm, LabelPx)> {
    let profile = resolve_profile(cfg, model, task)?;
    let label_mm = LabelMm::parse(&cfg.resolve_label(label))?;
    let lp = label_mm.to_pixels(profile.max_width_px);
    Ok((profile, label_mm, lp))
}

/// Save a rendered sticker, then print it unless `--no-print`.
struct RenderedOutput<'a> {
    save: Option<&'a PathBuf>,
    density: Density,
    no_print: bool,
    success: &'static str,
}

async fn save_and_print(
    cfg: &Config,
    conn: &ConnArgs,
    profile: PrintProfile,
    gray: &GrayImage,
    output: RenderedOutput<'_>,
) -> Result<()> {
    if let Some(png_path) = output.save {
        gray.save(png_path)
            .with_context(|| format!("save {}", png_path.display()))?;
        println!("saved {}", png_path.display());
    }
    if output.no_print {
        return Ok(());
    }
    print_gray_resolved(cfg, conn, profile, gray, output.density).await?;
    println!("{}", output.success);
    Ok(())
}

pub struct TextCommand {
    pub conn: ConnArgs,
    pub task: TaskArgs,
    pub font: FontArgs,
    pub model: Option<Model>,
    pub text: String,
    pub align: TextAlign,
    pub label: Option<String>,
    pub border: bool,
    pub density: Density,
    pub save: Option<PathBuf>,
    pub no_print: bool,
}

pub async fn text(cfg: &Config, args: TextCommand) -> Result<()> {
    let TextCommand {
        conn,
        task,
        font,
        model,
        text,
        align,
        label,
        border,
        density,
        save,
        no_print,
    } = args;
    let (profile, _label_mm, lp) = label_geometry(cfg, &task, model, label.as_deref())?;
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

    save_and_print(
        cfg,
        &conn,
        profile,
        &gray,
        RenderedOutput {
            save: save.as_ref(),
            density,
            no_print,
            success: "OK — text label printed",
        },
    )
    .await
}

pub struct QrCommand {
    pub conn: ConnArgs,
    pub task: TaskArgs,
    pub font: FontArgs,
    pub model: Option<Model>,
    pub url: String,
    pub text: String,
    pub text_side: TextSide,
    pub label: Option<String>,
    pub border: bool,
    pub density: Density,
    pub save: Option<PathBuf>,
    pub no_print: bool,
}

pub async fn qr(cfg: &Config, args: QrCommand) -> Result<()> {
    let QrCommand {
        conn,
        task,
        font,
        model,
        url,
        text,
        text_side,
        label,
        border,
        density,
        save,
        no_print,
    } = args;
    let (profile, _label_mm, lp) = label_geometry(cfg, &task, model, label.as_deref())?;
    let safe = cfg.resolve_safe_area();
    let gray = label::make_qr_label_opts(&QrLabelOptions {
        url,
        side_text: text.replace("\\n", "\n"),
        label: lp,
        safe,
        text_side,
        border,
        font_path: font.font.clone(),
        font_name: font.font_name.clone(),
        font_size: font.font_size,
    })?;
    info!(
        width_px = lp.width_px,
        height_px = lp.height_px,
        // Same `safe` the render used, so the log cannot disagree with the label.
        qr_side = label::max_qr_side(lp, safe),
        ?text_side,
        font_size = ?font.font_size,
        "qr label"
    );

    save_and_print(
        cfg,
        &conn,
        profile,
        &gray,
        RenderedOutput {
            save: save.as_ref(),
            density,
            no_print,
            success: "OK — QR label printed",
        },
    )
    .await
}

pub struct WifiCommand {
    pub conn: ConnArgs,
    pub task: TaskArgs,
    pub font: FontArgs,
    pub model: Option<Model>,
    pub ssid: String,
    pub password: String,
    pub security: WifiSecurity,
    pub hidden: bool,
    pub show_password: bool,
    pub text_side: TextSide,
    pub label: Option<String>,
    pub border: bool,
    pub density: Density,
    pub save: Option<PathBuf>,
    pub no_print: bool,
}

pub async fn wifi(cfg: &Config, args: WifiCommand) -> Result<()> {
    let WifiCommand {
        conn,
        task,
        font,
        model,
        ssid,
        password,
        security,
        hidden,
        show_password,
        text_side,
        label,
        border,
        density,
        save,
        no_print,
    } = args;
    let (profile, _label_mm, lp) = label_geometry(cfg, &task, model, label.as_deref())?;
    let password = resolve_wifi_password(password)?;
    if ssid.trim().chars().count() > 24 {
        eprintln!("tip: long SSID names wrap on a 50×30 sticker — shorter names read better");
    }
    // Never log the password (only SSID + security).
    info!(%ssid, ?security, hidden, show_password, "wifi sticker");

    let gray = make_wifi_label(&WifiLabelOptions {
        ssid: ssid.clone(),
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

    if let Some(png_path) = save.as_ref() {
        guard_wifi_save_path(png_path)?;
    }

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
        &conn,
        profile,
        &gray,
        RenderedOutput {
            save: save.as_ref(),
            density,
            no_print,
            success: "OK — Wi‑Fi sticker printed",
        },
    )
    .await
}
