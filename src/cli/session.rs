//! Opening a printer session and running one job through it.

use anyhow::{Context, Result, bail};
use std::path::Path;
use thermark::config::{Config, ConnPref};
use thermark::print_task::PrintTask;
use thermark::printer::{Pacing, PrintOptions, PrinterClient, PrinterSummary};
use thermark::protocol::Model;
use thermark::transport::{BleTransport, SerialTransport};

use super::args::{ConnArgs, ResolvedConn, TaskArgs};

/// Row pacing, overridable for diagnosing dense-page truncation.
///
/// `THERMARK_SLOW=1` selects [`Pacing::CAREFUL`]. Dense pages come back
/// truncated while sparse ones do not, which points at the printer dropping
/// data rather than at a printable-area limit; this makes that testable
/// without a rebuild.
fn pacing_from_env() -> Pacing {
    match std::env::var("THERMARK_SLOW") {
        Ok(v) if !v.trim().is_empty() && v != "0" => {
            eprintln!("pacing: CAREFUL (THERMARK_SLOW set)");
            Pacing::CAREFUL
        }
        _ => Pacing::REAL,
    }
}

/// An open BLE or USB printer session.
pub enum Session {
    Ble(PrinterClient<BleTransport>),
    Usb(PrinterClient<SerialTransport>),
}

/// Run the same call against whichever transport is open.
macro_rules! on_client {
    ($self:expr, $client:ident => $body:expr) => {
        match $self {
            Session::Ble($client) => $body,
            Session::Usb($client) => $body,
        }
    };
}

impl Session {
    pub async fn connect(conn: &ResolvedConn, model: Model, task: PrintTask) -> Result<Self> {
        match conn.conn {
            ConnPref::Ble => {
                let ble = BleTransport::connect_with(
                    &conn.addr,
                    std::time::Duration::from_secs(conn.scan_secs),
                    conn.match_mode,
                )
                .await
                .context("BLE connect")?;
                Ok(Self::Ble(
                    PrinterClient::new(ble, model)
                        .with_print_task(task)
                        .with_pacing(pacing_from_env()),
                ))
            }
            ConnPref::Usb => {
                let ser = SerialTransport::open(&conn.addr)
                    .with_context(|| format!("open serial {}", conn.addr))?;
                Ok(Self::Usb(
                    PrinterClient::new(ser, model)
                        .with_print_task(task)
                        .with_pacing(pacing_from_env()),
                ))
            }
        }
    }

    pub async fn fetch_summary(&mut self) -> Result<PrinterSummary> {
        Ok(on_client!(self, c => c.fetch_summary().await?))
    }

    pub async fn print_image_file_opts(&mut self, path: &Path, opts: PrintOptions) -> Result<()> {
        on_client!(self, c => c.print_image_file_opts(path, opts).await)?;
        Ok(())
    }

    /// Release the link. [`BleTransport`]'s `Drop` is only a backstop.
    pub async fn finish(self) {
        if let Self::Ble(c) = self {
            c.into_transport().disconnect().await.ok();
        }
    }
}

/// Connect, print one file, disconnect — the sequence every printing command runs.
///
/// Disconnect happens before the print result is propagated, so a failed job
/// still releases the BLE link (only one client may hold it at a time).
pub async fn print_file(
    cfg: &Config,
    conn: &ConnArgs,
    task_args: &TaskArgs,
    model: Model,
    path: &Path,
    opts: PrintOptions,
) -> Result<()> {
    let task = resolve_task(model, task_args)?;
    let conn = conn.resolve(cfg)?;
    let mut session = Session::connect(&conn, model, task).await?;
    let result = session.print_image_file_opts(path, opts).await;
    session.finish().await;
    result
}

/// Resolve the print task: `--task` wins, else `--simple-start`, else the
/// model default.
///
/// Non-B1 tasks require `--allow-experimental` so untested sequences are not
/// used by accident when a model default maps to one.
pub fn resolve_task(model: Model, args: &TaskArgs) -> Result<PrintTask> {
    let task = match (args.task, args.simple_start) {
        (Some(task), _) => task,
        (None, true) => PrintTask::Simple,
        (None, false) => PrintTask::for_model(model),
    };
    if !task.hardware_tested() {
        if !args.allow_experimental {
            bail!(
                "print task '{task}' is experimental (not hardware-tested in this project). \
                 Re-run with --allow-experimental if you accept the risk, \
                 or use --task b1 / --model b1. See: thermark tasks"
            );
        }
        eprintln!(
            "warning: print task '{task}' is experimental (not hardware-tested in this project)"
        );
    }
    Ok(task)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(task: Option<PrintTask>, simple: bool, allow: bool) -> TaskArgs {
        TaskArgs {
            simple_start: simple,
            task,
            allow_experimental: allow,
        }
    }

    #[test]
    fn explicit_task_beats_simple_start() {
        let t = resolve_task(Model::B1, &args(Some(PrintTask::B1), true, false)).unwrap();
        assert_eq!(t, PrintTask::B1);
    }

    #[test]
    fn experimental_requires_opt_in() {
        assert!(resolve_task(Model::B1, &args(Some(PrintTask::Simple), false, false)).is_err());
        assert!(resolve_task(Model::B1, &args(Some(PrintTask::Simple), false, true)).is_ok());
    }

    #[test]
    fn model_default_is_used_without_flags() {
        assert_eq!(
            resolve_task(Model::B1, &args(None, false, false)).unwrap(),
            PrintTask::B1
        );
        // D110 maps to an experimental task, so it is gated too.
        assert!(resolve_task(Model::D110, &args(None, false, false)).is_err());
    }
}
