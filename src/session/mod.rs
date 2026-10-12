//! Opening a printer session and running one job through it.

use crate::config::{Config, ConnPref};
use crate::print_task::PrintTask;
use crate::printer::Pacing;
use crate::printer::{PrintOptions, PrinterClient, PrinterSummary};
use crate::profile::PrinterProfile;
use crate::protocol::Model;
#[cfg(feature = "ble")]
use crate::transport::BleTransport;
#[cfg(feature = "serial")]
use crate::transport::SerialTransport;
use crate::transport::Transport;
#[cfg(any(feature = "ble", feature = "serial"))]
use anyhow::Context;
use anyhow::{Result, bail};
use std::path::Path;

/// Resolved connection settings; environment/config resolution belongs to the caller.
pub struct ConnectionOptions {
    pub conn: ConnPref,
    pub addr: String,
    pub scan_secs: u64,
    pub match_mode: crate::transport::BleMatchMode,
    pub pacing: Pacing,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TargetOptions {
    pub task: Option<PrintTask>,
    pub allow_experimental: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum TaskSelection {
    /// Use this provisional default until printer identification selects its task.
    Auto {
        default: PrintTask,
    },
    Explicit(PrintTask),
}

impl TaskSelection {
    fn initial_task(self) -> PrintTask {
        match self {
            Self::Auto { default } => default,
            Self::Explicit(task) => task,
        }
    }

    fn uses_detected_default(self) -> bool {
        matches!(self, Self::Auto { .. })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PrintTarget {
    pub model: Model,
    pub task: TaskSelection,
    pub allow_experimental: bool,
}

pub fn resolve_target(
    cfg: &Config,
    model: Option<Model>,
    args: &TargetOptions,
) -> Result<PrintTarget> {
    let model = cfg.resolve_model(model);
    let task = resolve_task(model, args)?;
    Ok(PrintTarget {
        model,
        task: match args.task {
            Some(task) => TaskSelection::Explicit(task),
            None => TaskSelection::Auto { default: task },
        },
        allow_experimental: args.allow_experimental,
    })
}

/// An open BLE or USB printer session.
pub struct Session<T: Transport = AnyTransport> {
    client: PrinterClient<T>,
    connection: ConnPref,
    allow_experimental: bool,
}

#[derive(Debug, Clone, Copy)]
enum IdentityDetail {
    Profile,
    Full,
}

pub fn combine_job_and_close<T>(job: Result<T>, close: Result<()>) -> Result<T> {
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

impl<T: Transport> Session<T> {
    /// Identify a caller-provided transport, retaining the same print policy as the CLI.
    pub async fn from_transport(
        transport: T,
        connection: ConnPref,
        target: PrintTarget,
        pacing: Pacing,
    ) -> Result<Self> {
        let client =
            PrinterClient::new_with_task(transport, target.model, target.task.initial_task())
                .with_pacing(pacing);
        Self::finish_connect(client, connection, target, IdentityDetail::Profile).await
    }

    pub fn print_task(&self) -> PrintTask {
        self.client.print_task()
    }

    #[cfg_attr(not(any(feature = "ble", feature = "serial")), allow(dead_code))]
    async fn finish_connect(
        mut client: PrinterClient<T>,
        connection: ConnPref,
        target: PrintTarget,
        identity_detail: IdentityDetail,
    ) -> Result<Self> {
        let identity_result = match identity_detail {
            IdentityDetail::Profile => client.identify_profile().await,
            IdentityDetail::Full => client.identify().await,
        };
        let identity = match identity_result {
            Ok(identity) => Some(identity),
            Err(error) => {
                tracing::warn!(error = %error, "printer identification failed");
                None
            }
        };
        if let Some(identity) = &identity {
            if let Some(profile) =
                client.apply_identity(identity, target.task.uses_detected_default())
            {
                tracing::info!(model = %profile.model, model_id = identity.model_id, dpi = profile.dpi, task = ?profile.default_task, "identified printer");
            } else {
                tracing::warn!(
                    model_id = identity.model_id,
                    "printer model is not in the profile registry"
                );
            }
        }
        Ok(Self {
            client,
            connection,
            allow_experimental: target.allow_experimental,
        })
    }

    pub fn identity(&self) -> Option<&crate::PrinterIdentity> {
        self.client.identity()
    }

    pub fn profile(&self) -> &'static PrinterProfile {
        self.client.profile()
    }

    pub async fn fetch_summary(&mut self) -> Result<PrinterSummary> {
        Ok(self.client.fetch_summary().await?)
    }

    pub async fn print_image_file_opts(&mut self, path: &Path, opts: PrintOptions) -> Result<()> {
        self.ensure_print_allowed()?;
        self.client.print_image_file_opts(path, opts).await?;
        Ok(())
    }

    /// Render against the connected printer's effective physical profile,
    /// then print the exact grayscale image that renderer produced.
    ///
    /// Identification and the experimental-path gate both happen before the
    /// renderer is called, so DPI-dependent geometry cannot be derived from a
    /// stale configured model. The caller owns closing the session so the same
    /// shutdown path covers render, save, and print failures.
    pub async fn render_and_print_gray<R>(
        &mut self,
        density: crate::Density,
        render: impl FnOnce(&'static PrinterProfile) -> Result<(image::GrayImage, R)>,
    ) -> Result<R> {
        self.ensure_print_allowed()?;
        let (gray, rendered) = render(self.client.profile())?;
        self.client.print_gray_image(&gray, density).await?;
        Ok(rendered)
    }

    pub fn ensure_print_allowed(&self) -> Result<()> {
        let identity = self.client.identity().ok_or_else(|| {
            anyhow::anyhow!(
                "printer identification failed; refusing to print with provisional geometry"
            )
        })?;
        if crate::profile_for_identity(identity).is_none() {
            bail!(
                "unrecognized printer model id {}; add a checked device profile before printing",
                identity.model_id
            );
        }
        let model = self.client.model();
        let task = self.client.print_task();
        ensure_print_path_allowed(model, task, self.connection, self.allow_experimental)?;
        if !crate::profile_for_model(model).print_path_hardware_tested(task, self.connection) {
            let connection = self.connection;
            eprintln!(
                "warning: printer profile '{model}' with print task '{task}' over '{connection}' is experimental \
                 (not hardware-tested in this project)"
            );
        }
        Ok(())
    }

    pub async fn print_gray_with_progress(
        &mut self,
        gray: &image::GrayImage,
        density: crate::Density,
        progress: impl FnMut(crate::printer::PrintStage) -> crate::Result<()>,
    ) -> Result<()> {
        self.ensure_print_allowed()?;
        let raster = crate::image_encode::encode_gray(
            gray,
            self.client.max_width_px(),
            crate::Threshold::DEFAULT.get(),
            false,
        )?;
        self.client.preflight_ready().await?;
        Ok(self
            .client
            .print_raster_with_progress(raster, density, progress)
            .await?)
    }

    /// Release the link. `BleTransport`'s `Drop` is only a backstop.
    pub async fn finish(self) -> Result<()> {
        self.client.close().await.map_err(anyhow::Error::from)
    }

    pub async fn print_gray(
        &mut self,
        gray: &image::GrayImage,
        density: crate::Density,
    ) -> Result<()> {
        self.ensure_print_allowed()?;
        Ok(self.client.print_gray_image(gray, density).await?)
    }
}

pub async fn run_rendered_session<T: Transport, R>(
    mut session: Session<T>,
    density: crate::Density,
    render: impl FnOnce(&'static PrinterProfile) -> Result<(image::GrayImage, R)>,
) -> Result<R> {
    let result = session.render_and_print_gray(density, render).await;
    let close_result = session.finish().await;
    combine_job_and_close(result, close_result)
}

fn connected_safe_area(
    cfg: &Config,
    profile: &PrinterProfile,
    full_bleed: bool,
) -> crate::geometry::SafeArea {
    if full_bleed {
        crate::geometry::SafeArea::NONE
    } else {
        cfg.resolve_safe_area(profile.pixels_per_mm())
    }
}

pub async fn run_file_session<T: Transport>(
    mut session: Session<T>,
    cfg: &Config,
    path: &Path,
    mut opts: PrintOptions,
    full_bleed: bool,
) -> Result<()> {
    // A saved safe area is already expressed in pixels and remains exact.
    // The default registration inset is physical (1 mm), so resolve it again
    // at the detected DPI rather than retaining the provisional model's value.
    opts.safe = connected_safe_area(cfg, session.profile(), full_bleed);
    let result = session.print_image_file_opts(path, opts).await;
    let close_result = session.finish().await;
    combine_job_and_close(result, close_result)
}

/// Resolve the print task: `--task` wins, otherwise use the profile default.
///
/// This only selects protocol metadata. The experimental-task gate belongs at
/// the actual printing boundary so offline previews and renders can use every
/// model profile without opting in to hardware writes.
pub fn resolve_task(model: Model, args: &TargetOptions) -> Result<PrintTask> {
    args.task.or_else(|| PrintTask::for_model(model)).ok_or_else(|| {
        anyhow::anyhow!(
            "model '{model}' has no verified default print task; identify it and pass --task explicitly"
        )
    })
}

#[cfg(test)]
mod tests;

mod connection;
pub use connection::AnyTransport;

mod policy;
use policy::ensure_print_path_allowed;
pub use policy::ensure_target_print_allowed;
