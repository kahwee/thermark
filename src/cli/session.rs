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
    pub pixels_per_mm: f64,
    pub task_explicit: bool,
    pub allow_experimental: bool,
}

pub fn resolve_profile(
    cfg: &Config,
    model: Option<Model>,
    args: &TaskArgs,
) -> Result<PrintProfile> {
    let model = cfg.resolve_model(model);
    let task = resolve_task(model, args)?;
    let device = thermark::profile_for_model(model);
    Ok(PrintProfile {
        model,
        task,
        max_width_px: device.max_width_px,
        pixels_per_mm: device.pixels_per_mm(),
        task_explicit: args.task.is_some(),
        allow_experimental: args.allow_experimental,
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
pub struct Session {
    client: PrinterClient<AnyTransport>,
    allow_experimental: bool,
}

#[derive(Debug, Clone, Copy)]
enum IdentityDetail {
    Profile,
    Full,
}

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
    pub async fn connect(
        conn: &ResolvedConn,
        model: Model,
        task: PrintTask,
        auto_task: bool,
        allow_experimental: bool,
    ) -> Result<Self> {
        Self::connect_with_identity(
            conn,
            model,
            task,
            auto_task,
            allow_experimental,
            IdentityDetail::Profile,
        )
        .await
    }

    /// Open a session and retain the full identity report for presentation.
    ///
    /// Normal printing only needs [`Self::connect`]; this variant keeps the
    /// firmware and hardware metadata queries used by the `identify` command.
    pub async fn connect_detailed(
        conn: &ResolvedConn,
        model: Model,
        task: PrintTask,
        auto_task: bool,
        allow_experimental: bool,
    ) -> Result<Self> {
        Self::connect_with_identity(
            conn,
            model,
            task,
            auto_task,
            allow_experimental,
            IdentityDetail::Full,
        )
        .await
    }

    #[allow(unused_variables)]
    async fn connect_with_identity(
        conn: &ResolvedConn,
        model: Model,
        task: PrintTask,
        auto_task: bool,
        allow_experimental: bool,
        identity_detail: IdentityDetail,
    ) -> Result<Self> {
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
                    let client = PrinterClient::new_with_task(AnyTransport::Ble(ble), model, task)
                        .with_pacing(pacing_from_env());
                    Self::finish_connect(client, auto_task, allow_experimental, identity_detail)
                        .await
                }
            }
            ConnPref::Usb => {
                #[cfg(not(feature = "serial"))]
                bail!("this thermark binary was built without USB serial support");
                #[cfg(feature = "serial")]
                {
                    let ser = SerialTransport::open(&conn.addr)
                        .with_context(|| format!("open serial {}", conn.addr))?;
                    let client = PrinterClient::new_with_task(AnyTransport::Usb(ser), model, task)
                        .with_pacing(pacing_from_env());
                    Self::finish_connect(client, auto_task, allow_experimental, identity_detail)
                        .await
                }
            }
        }
    }

    #[cfg(any(feature = "ble", feature = "serial"))]
    async fn finish_connect(
        mut client: PrinterClient<AnyTransport>,
        auto_task: bool,
        allow_experimental: bool,
        identity_detail: IdentityDetail,
    ) -> Result<Self> {
        let identity = match identity_detail {
            IdentityDetail::Profile => client.identify_profile().await,
            IdentityDetail::Full => client.identify().await,
        }
        .ok();
        if let Some(identity) = &identity {
            if let Some(profile) = client.apply_identity(identity, auto_task) {
                tracing::info!(model = %profile.model, model_id = identity.model_id, dpi = profile.dpi, task = ?profile.task, "identified printer");
            } else {
                tracing::warn!(
                    model_id = identity.model_id,
                    "printer model is not in the profile registry"
                );
            }
        }
        Ok(Self {
            client,
            allow_experimental,
        })
    }

    pub fn identity(&self) -> Option<&thermark::PrinterIdentity> {
        self.client.identity()
    }

    pub async fn fetch_summary(&mut self) -> Result<PrinterSummary> {
        Ok(self.client.fetch_summary().await?)
    }

    pub async fn print_image_file_opts(&mut self, path: &Path, opts: PrintOptions) -> Result<()> {
        self.ensure_print_allowed()?;
        self.client.print_image_file_opts(path, opts).await?;
        Ok(())
    }

    pub async fn print_gray_image(
        &mut self,
        gray: &image::GrayImage,
        density: thermark::Density,
    ) -> Result<()> {
        self.ensure_print_allowed()?;
        self.client.preflight_ready().await?;
        self.client.print_gray_image(gray, density).await?;
        Ok(())
    }

    fn ensure_print_allowed(&self) -> Result<()> {
        let task = self.client.print_task();
        if !task.hardware_tested() && !self.allow_experimental {
            bail!(
                "detected printer uses experimental task '{task}'. Re-run with \
                 --allow-experimental if you accept the risk"
            );
        }
        Ok(())
    }

    /// Release the link. [`BleTransport`]'s `Drop` is only a backstop.
    pub async fn finish(self) -> Result<()> {
        self.client.close().await.map_err(anyhow::Error::from)
    }
}

/// Connect, print one file, disconnect — the sequence every printing command runs.
///
/// Disconnect happens before the print result is propagated, so a failed job
/// still releases the BLE link (only one client may hold it at a time).
pub async fn print_file_resolved(
    cfg: &Config,
    conn: &ConnArgs,
    profile: PrintProfile,
    path: &Path,
    opts: PrintOptions,
) -> Result<()> {
    let conn = conn.resolve(cfg)?;
    let mut session = Session::connect(
        &conn,
        profile.model,
        profile.task,
        !profile.task_explicit,
        profile.allow_experimental,
    )
    .await?;
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
    let mut session = Session::connect(
        &resolved_conn,
        profile.model,
        profile.task,
        !profile.task_explicit,
        profile.allow_experimental,
    )
    .await?;
    let result = session.print_gray_image(gray, density).await;
    let close_result = session.finish().await;
    combine_job_and_close(result, close_result)
}

/// Resolve the print task: `--task` wins, otherwise use the profile default.
///
/// Non-B1 tasks require `--allow-experimental` so untested sequences are not
/// used by accident when a model default maps to one.
pub fn resolve_task(model: Model, args: &TaskArgs) -> Result<PrintTask> {
    let task = args.task.or_else(|| PrintTask::for_model(model)).ok_or_else(|| {
        anyhow::anyhow!(
            "model '{model}' has no verified default print task; identify it and pass --task explicitly"
        )
    })?;
    if !task.hardware_tested() {
        if !args.allow_experimental {
            bail!(
                "print task '{task}' is experimental (not hardware-tested in this project). \
                 Re-run with --allow-experimental if you accept the risk, \
                 or use a hardware-tested profile. See: thermark tasks"
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

    fn args(task: Option<PrintTask>, allow: bool) -> TaskArgs {
        TaskArgs {
            task,
            allow_experimental: allow,
        }
    }

    #[test]
    fn explicit_task_is_used() {
        let t = resolve_task(Model::B1, &args(Some(PrintTask::B1), false)).unwrap();
        assert_eq!(t, PrintTask::B1);
    }

    #[test]
    fn experimental_requires_opt_in() {
        assert!(resolve_task(Model::B1, &args(Some(PrintTask::D110), false)).is_err());
        assert!(resolve_task(Model::B1, &args(Some(PrintTask::D110), true)).is_ok());
    }

    #[test]
    fn model_default_is_used_without_flags() {
        assert_eq!(
            resolve_task(Model::B1, &args(None, false)).unwrap(),
            PrintTask::B1
        );
        // D110 maps to an experimental task, so it is gated too.
        assert!(resolve_task(Model::D110, &args(None, false)).is_err());
    }
}
