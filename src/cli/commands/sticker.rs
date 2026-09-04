//! Composed stickers: `qr` and `wifi`.
//!
//! Each command renders in memory, optionally saves a PNG, then optionally
//! prints through the shared output path.

use anyhow::{Context, Result};
use image::GrayImage;
use std::path::PathBuf;
use thermark::config::Config;
use thermark::geometry::LabelMm;
use thermark::label::{self, QrLabelOptions, TextLabelOptions, make_text_label};
use thermark::profile::PrinterProfile;
use thermark::protocol::Model;
use thermark::types::Density;
use thermark::wifi::{WifiLabelOptions, make_wifi_label};
use tracing::info;

use crate::cli::args::{ConnArgs, QrCommand, TaskArgs, TextCommand, WifiCommand};
use crate::cli::session::{PrintTarget, render_and_print_gray_resolved, resolve_target};
use crate::cli::tips::{guard_wifi_save_path, resolve_wifi_password};

/// Resolve only profile-independent label input before opening a connection.
/// Pixel geometry is deliberately deferred until printer identification.
fn label_request(
    cfg: &Config,
    task: &TaskArgs,
    model: Option<Model>,
    label: Option<&str>,
) -> Result<(PrintTarget, LabelMm)> {
    let target = resolve_target(cfg, model, task)?;
    let label_mm = LabelMm::parse(&cfg.resolve_label(label))?;
    Ok((target, label_mm))
}

/// Save a rendered sticker, then print it unless `--no-print`.
struct RenderedOutput<'a> {
    save: Option<&'a PathBuf>,
    density: Density,
    no_print: bool,
    success: &'static str,
}

fn save_rendered(gray: &GrayImage, path: Option<&PathBuf>) -> Result<()> {
    if let Some(png_path) = path {
        gray.save(png_path)
            .with_context(|| format!("save {}", png_path.display()))?;
        println!("saved {}", png_path.display());
    }
    Ok(())
}

async fn render_save_and_print(
    cfg: &Config,
    conn: &ConnArgs,
    target: PrintTarget,
    output: RenderedOutput<'_>,
    render: impl FnOnce(&'static PrinterProfile) -> Result<GrayImage>,
) -> Result<()> {
    if output.no_print {
        let gray = render(thermark::profile_for_model(target.model))?;
        save_rendered(&gray, output.save)?;
        return Ok(());
    }
    render_and_print_gray_resolved(cfg, conn, target, output.density, |detected| {
        let gray = render(detected)?;
        save_rendered(&gray, output.save)?;
        Ok((gray, ()))
    })
    .await?;
    println!("{}", output.success);
    Ok(())
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
    let (target, label_mm) = label_request(cfg, &task, model, label.as_deref())?;

    render_save_and_print(
        cfg,
        &conn,
        target,
        RenderedOutput {
            save: save.as_ref(),
            density,
            no_print,
            success: "OK — text label printed",
        },
        |detected| {
            let lp = label_mm.to_pixels(detected.max_width_px, detected.pixels_per_mm());
            let gray = make_text_label(&TextLabelOptions {
                text: text.replace("\\n", "\n"),
                label: lp,
                safe: cfg.resolve_safe_area(detected.pixels_per_mm()),
                align,
                border,
                font_path: font.font,
                font_name: font.font_name,
                font_size: font.font_size,
            })?;
            info!(
                model = %detected.model,
                dpi = detected.dpi,
                width_px = lp.width_px,
                height_px = lp.height_px,
                ?align,
                font_size = ?font.font_size,
                "text label"
            );
            Ok(gray)
        },
    )
    .await
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
    let (target, label_mm) = label_request(cfg, &task, model, label.as_deref())?;

    render_save_and_print(
        cfg,
        &conn,
        target,
        RenderedOutput {
            save: save.as_ref(),
            density,
            no_print,
            success: "OK — QR label printed",
        },
        |detected| {
            let lp = label_mm.to_pixels(detected.max_width_px, detected.pixels_per_mm());
            let safe = cfg.resolve_safe_area(detected.pixels_per_mm());
            let gray = label::make_qr_label_opts(&QrLabelOptions {
                url,
                side_text: text.replace("\\n", "\n"),
                label: lp,
                safe,
                text_side,
                border,
                font_path: font.font,
                font_name: font.font_name,
                font_size: font.font_size,
            })?;
            info!(
                model = %detected.model,
                dpi = detected.dpi,
                width_px = lp.width_px,
                height_px = lp.height_px,
                // Same `safe` the render used, so the log cannot disagree with the label.
                qr_side = label::max_qr_side(lp, safe),
                ?text_side,
                font_size = ?font.font_size,
                "qr label"
            );
            Ok(gray)
        },
    )
    .await
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
    let (target, label_mm) = label_request(cfg, &task, model, label.as_deref())?;
    // Open networks have no credential to resolve. In particular, do not read
    // THERMARK_WIFI_PASSWORD or require --password for `--security nopass`.
    let password = if security.requires_password() {
        resolve_wifi_password(password)?
    } else {
        String::new()
    };
    if ssid.trim().chars().count() > 24 {
        eprintln!("tip: long SSID names wrap on a 50×30 sticker — shorter names read better");
    }
    // Never log the password (only SSID + security).
    info!(%ssid, ?security, hidden, show_password, "wifi sticker");

    if let Some(png_path) = save.as_ref() {
        guard_wifi_save_path(png_path)?;
    }

    println!("SSID on sticker: {ssid}");
    if !security.requires_password() {
        println!("security: open network (no password)");
    } else if show_password {
        eprintln!(
            "warning: password is printed in cleartext on the label \
             (anyone who sees the sticker can read it)"
        );
    } else {
        println!("password: in QR only (not printed as text)");
    }

    render_save_and_print(
        cfg,
        &conn,
        target,
        RenderedOutput {
            save: save.as_ref(),
            density,
            no_print,
            success: "OK — Wi‑Fi sticker printed",
        },
        |detected| {
            let lp = label_mm.to_pixels(detected.max_width_px, detected.pixels_per_mm());
            let gray = make_wifi_label(&WifiLabelOptions {
                ssid: ssid.clone(),
                password,
                security,
                hidden,
                show_password,
                label: lp,
                safe: cfg.resolve_safe_area(detected.pixels_per_mm()),
                text_side,
                font_path: font.font,
                font_name: font.font_name,
                font_size: font.font_size,
                border,
            })
            .with_context(|| {
                "building Wi‑Fi sticker (if QR failed: password may be too long for a dense code)"
            })?;
            info!(
                model = %detected.model,
                dpi = detected.dpi,
                width_px = lp.width_px,
                height_px = lp.height_px,
                "wifi label"
            );
            Ok(gray)
        },
    )
    .await
}
