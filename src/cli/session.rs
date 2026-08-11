//! Opening a printer session and running one job through it.

#[cfg(any(feature = "ble", feature = "serial"))]
use anyhow::Context;
use anyhow::{Result, bail};
use std::path::Path;
use thermark::config::{Config, ConnPref};
use thermark::print_task::PrintTask;
#[cfg(any(feature = "ble", feature = "serial"))]
use thermark::printer::Pacing;
use thermark::printer::{PrintOptions, PrinterClient, PrinterSummary};
use thermark::protocol::Model;
#[cfg(feature = "ble")]
use thermark::transport::BleTransport;
#[cfg(feature = "serial")]
use thermark::transport::SerialTransport;
use thermark::transport::Transport;

use super::args::{ConnArgs, ResolvedConn, TaskArgs};

#[derive(Debug, Clone, Copy)]
pub struct PrintProfile {
    pub model: Model,
    pub task: PrintTask,
    pub max_width_px: u32,
}

pub fn resolve_profile(
    cfg: &Config,
    model: Option<Model>,
    args: &TaskArgs,
) -> Result<PrintProfile> {
    let model = cfg.resolve_model(model);
    let task = resolve_task(model, args)?;
    Ok(PrintProfile {
        model,
        task,
        max_width_px: thermark::effective_max_width_px(model, task),
    })
}

/// Row pacing, overridable for diagnosing dense-page truncation.
///
/// `THERMARK_SLOW=1` selects [`Pacing::CAREFUL`]. Dense pages come back
/// truncated while sparse ones do not, which points at the printer dropping
/// data rather than at a printable-area limit; this makes that testable
/// without a rebuild.
#[cfg(any(feature = "ble", feature = "serial"))]
fn pacing_from_env() -> Pacing {
    match std::env::var("THERMARK_SLOW") {
        Ok(v) if !v.trim().is_empty() && v != "0" => {
            eprintln!("pacing: CAREFUL (THERMARK_SLOW set)");
            Pacing::CAREFUL
        }
        _ => Pacing::REAL,
    }
}

/// CLI transport sum type; the protocol client remains transport-agnostic.
pub enum AnyTransport {
    #[cfg(feature = "ble")]
    Ble(BleTransport),
    #[cfg(feature = "serial")]
    Usb(SerialTransport),
}

impl Transport for AnyTransport {
    #[allow(unused_variables)]
    async fn send_raw(&mut self, data: &[u8]) -> thermark::Result<()> {
        match self {
            #[cfg(feature = "ble")]
            Self::Ble(transport) => transport.send_raw(data).await,
            #[cfg(feature = "serial")]
            Self::Usb(transport) => transport.send_raw(data).await,
            #[cfg(not(any(feature = "ble", feature = "serial")))]
            _ => unreachable!("AnyTransport has no variants"),
        }
    }

    #[allow(unused_variables)]
    async fn recv_raw(&mut self, wait: std::time::Duration) -> thermark::Result<Vec<u8>> {
        match self {
            #[cfg(feature = "ble")]
            Self::Ble(transport) => transport.recv_raw(wait).await,
            #[cfg(feature = "serial")]
            Self::Usb(transport) => transport.recv_raw(wait).await,
            #[cfg(not(any(feature = "ble", feature = "serial")))]
            _ => unreachable!("AnyTransport has no variants"),
        }
    }

    async fn close(&mut self) -> thermark::Result<()> {
        match self {
            #[cfg(feature = "ble")]
            Self::Ble(transport) => transport.close().await,
            #[cfg(feature = "serial")]
            Self::Usb(transport) => transport.close().await,
            #[cfg(not(any(feature = "ble", feature = "serial")))]
            _ => unreachable!("AnyTransport has no variants"),
        }
    }
}

/// An open BLE or USB printer session.
pub struct Session(PrinterClient<AnyTransport>);

fn combine_job_and_close<T>(job: Result<T>, close: Result<()>) -> Result<T> {
    match (job, close) {
        (Err(job), Err(close)) => {
            tracing::warn!(error = %close, "printer shutdown failed after operation error");
            Err(job)
        }
        (Err(job), Ok(())) => Err(job),
        (Ok(_), Err(close)) => Err(close),
        (Ok(value), Ok(())) => Ok(value),
    }
}

impl Session {
    #[allow(unused_variables)]
    pub async fn connect(conn: &ResolvedConn, model: Model, task: PrintTask) -> Result<Self> {
        match conn.conn {
            ConnPref::Ble => {
                #[cfg(not(feature = "ble"))]
                bail!("this thermark binary was built without Bluetooth support");
                #[cfg(feature = "ble")]
                {
                    let ble = BleTransport::connect_with(
                        &conn.addr,
                        std::time::Duration::from_secs(conn.scan_secs),
                        conn.match_mode,
                    )
                    .await
                    .context("BLE connect")?;
                    Ok(Self(
                        PrinterClient::new(AnyTransport::Ble(ble), model)
                            .with_print_task(task)
                            .with_pacing(pacing_from_env()),
                    ))
                }
            }
            ConnPref::Usb => {
                #[cfg(not(feature = "serial"))]
                bail!("this thermark binary was built without USB serial support");
                #[cfg(feature = "serial")]
                {
                    let ser = SerialTransport::open(&conn.addr)
                        .with_context(|| format!("open serial {}", conn.addr))?;
                    Ok(Self(
                        PrinterClient::new(AnyTransport::Usb(ser), model)
                            .with_print_task(task)
                            .with_pacing(pacing_from_env()),
                    ))
                }
            }
        }
    }

    pub async fn fetch_summary(&mut self) -> Result<PrinterSummary> {
        Ok(self.0.fetch_summary().await?)
    }

    pub async fn print_image_file_opts(&mut self, path: &Path, opts: PrintOptions) -> Result<()> {
        self.0.print_image_file_opts(path, opts).await?;
        Ok(())
    }

    pub async fn print_gray_image(
        &mut self,
        gray: &image::GrayImage,
        density: thermark::Density,
    ) -> Result<()> {
        if let Ok(rfid) = self.0.rfid_info().await {
            tracing::info!(%rfid, "RFID");
        }
        self.0.preflight_ready().await?;
        self.0.print_gray_image(gray, density).await?;
        Ok(())
    }

    /// Release the link. [`BleTransport`]'s `Drop` is only a backstop.
    pub async fn finish(self) -> Result<()> {
        self.0.close().await.map_err(anyhow::Error::from)
    }
}

/// Connect, print one file, disconnect — the sequence every printing command runs.
///
/// Disconnect happens before the print result is propagated, so a failed job
/// still releases the BLE link (only one client may hold it at a time).
pub async fn print_file_resolved(
    cfg: &Config,
    conn: &ConnArgs,
    model: Model,
    task: PrintTask,
    path: &Path,
    opts: PrintOptions,
) -> Result<()> {
    let conn = conn.resolve(cfg)?;
    let mut session = Session::connect(&conn, model, task).await?;
    let result = session.print_image_file_opts(path, opts).await;
    let close_result = session.finish().await;
    combine_job_and_close(result, close_result)
}

pub async fn print_gray_resolved(
    cfg: &Config,
    conn: &ConnArgs,
    profile: PrintProfile,
    gray: &image::GrayImage,
    density: thermark::Density,
) -> Result<()> {
    let resolved_conn = conn.resolve(cfg)?;
    let mut session = Session::connect(&resolved_conn, profile.model, profile.task).await?;
    let result = session.print_gray_image(gray, density).await;
    let close_result = session.finish().await;
    combine_job_and_close(result, close_result)
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
