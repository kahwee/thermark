//! The `thermark` command-line application.
//!
//! Layout:
//! - [`args`] — clap types and the argument groups shared between commands
//! - [`session`] — connecting, running one job, releasing the link
//! - [`commands`] — one module per command group
//! - [`tips`] — advisory stderr output (never changes behaviour)

pub mod args;
pub mod commands;
pub mod session;
pub mod tips;

use anyhow::Result;
use args::{Cli, Commands};
use thermark::config::Config;

/// Dispatch a parsed command. Returns the process exit code.
pub async fn run(cli: Cli) -> Result<i32> {
    let cfg = Config::load().unwrap_or_default();

    match cli.command {
        Commands::Scan {
            seconds,
            save,
            name,
        } => commands::device::scan(seconds, save, name.as_deref()).await?,
        Commands::Ports => commands::device::ports()?,
        Commands::Info { conn, model } => commands::device::info(&cfg, &conn, model).await?,
        Commands::Fonts => commands::device::fonts(),
        Commands::Tasks => commands::device::tasks(),
        Commands::Encode { cmd, data } => commands::device::encode(&cmd, &data)?,
        Commands::Config { action } => commands::config::run(action)?,

        Commands::Print {
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
        } => {
            commands::print::print(
                &cfg, &conn, &task, &image, model, density, rotate, threshold, fit, label, fill,
                no_fill, margin, dither, no_trim, full_bleed, preview,
            )
            .await?
        }

        Commands::Calibrate {
            conn,
            task,
            model,
            label,
            density,
            boundary,
        } => {
            commands::print::calibrate(&cfg, &conn, &task, model, &label, density, boundary).await?
        }

        Commands::Text {
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
        } => {
            commands::sticker::text(
                &cfg, &conn, &task, &font, model, &text, align, &label, border, density, save,
                no_print,
            )
            .await?
        }

        Commands::Qr {
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
        } => {
            commands::sticker::qr(
                &cfg, &conn, &task, &font, model, &url, &text, text_side, &label, border, density,
                save, no_print,
            )
            .await?
        }

        Commands::Wifi {
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
        } => {
            commands::sticker::wifi(
                &cfg,
                &conn,
                &task,
                &font,
                model,
                &ssid,
                password,
                security,
                hidden,
                show_password,
                text_side,
                &label,
                border,
                density,
                save,
                no_print,
            )
            .await?
        }

        Commands::Doctor {
            addr,
            conn,
            model,
            task,
            seconds,
            use_config,
            fuzzy,
        } => {
            return commands::doctor::run(
                &cfg, addr, conn, model, task, seconds, use_config, fuzzy,
            )
            .await;
        }
    }

    Ok(0)
}
