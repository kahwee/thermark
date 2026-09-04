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

/// Handle commands that need neither async I/O nor the printer runtime.
///
/// Keeping this check before Tokio/tracing initialization makes quick
/// inspection and packet-encoding commands behave like small Unix tools.
pub fn run_sync(cli: &Cli) -> Option<Result<i32>> {
    let result = match &cli.command {
        Commands::Ports => commands::device::ports(),
        Commands::Fonts => {
            commands::device::fonts();
            Ok(())
        }
        Commands::Tasks => {
            commands::device::tasks();
            Ok(())
        }
        Commands::Encode { cmd, data } => commands::device::encode(cmd, data),
        Commands::Config { action } => commands::config::run(action),
        _ => return None,
    };
    Some(result.map(|()| 0))
}

/// Dispatch a parsed command. Returns the process exit code.
pub async fn run(cli: Cli) -> Result<i32> {
    // `main` takes this path before creating Tokio. Keep the guard here too so
    // direct callers still share the same dispatcher instead of duplicating
    // the synchronous command implementations below.
    if let Some(result) = run_sync(&cli) {
        return result;
    }

    match cli.command {
        Commands::Scan {
            seconds,
            save,
            name,
        } => commands::device::scan(seconds, save, name.as_deref()).await?,
        Commands::Info { conn } => {
            let cfg = Config::load()?;
            commands::device::info(&cfg, &conn).await?
        }
        Commands::Identify { conn, json } => {
            let cfg = Config::load()?;
            commands::device::identify(&cfg, &conn, json).await?
        }
        Commands::Print(args) => {
            let cfg = Config::load()?;
            commands::print::print(&cfg, args).await?
        }
        Commands::Calibrate(args) => {
            let cfg = Config::load()?;
            commands::print::calibrate(&cfg, args).await?
        }
        Commands::Text(args) => {
            let cfg = Config::load()?;
            commands::sticker::text(&cfg, args).await?
        }
        Commands::Qr(args) => {
            let cfg = Config::load()?;
            commands::sticker::qr(&cfg, args).await?
        }
        Commands::Wifi(args) => {
            let cfg = Config::load()?;
            commands::sticker::wifi(&cfg, args).await?
        }
        Commands::Doctor(args) => {
            let cfg = Config::load()?;
            return commands::doctor::run(&cfg, args).await;
        }
        Commands::Ports
        | Commands::Fonts
        | Commands::Tasks
        | Commands::Encode { .. }
        | Commands::Config { .. } => {
            unreachable!("synchronous commands are dispatched before starting the runtime")
        }
    }

    Ok(0)
}
