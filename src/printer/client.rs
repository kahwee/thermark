//! Printer client state and safe lifecycle entry points.

use super::pacing::Pacing;
use super::raw::RawPrinter;
use crate::errors::{Error, Result};
use crate::packet::PacketDecoder;
use crate::print_task::{PrintTask, effective_max_width_px};
use crate::protocol::Model;
use crate::transport::Transport;
use std::time::Duration;
use tracing::{info, warn};

pub struct PrinterClient<T: Transport> {
    pub(crate) transport: T,
    pub(crate) model: Model,
    pub(crate) task: PrintTask,
    pub(crate) pacing: Pacing,
    pub(crate) decoder: PacketDecoder,
}

/// Power level at or below which dense pages become unreliable.
pub const LOW_BATTERY_LEVEL: u8 = 1;

/// Warn before printing on a low battery.
///
/// A dense page fires far more heating elements than a sparse one, so it draws
/// much more current. On a low battery that sags the supply and the printer
/// stops part-way through, which looks exactly like a clipped label. Observed
/// on a real B1 at level 1: a 14.6 KB page truncated around 73% while an 8.2 KB
/// page on the same roll completed, and repeat runs of the *same* page differed
/// — the tell that it is power, not data, since no buffer or pacing model
/// produces run-to-run variation.
///
/// Only an empty battery blocks printing
/// ([`super::Heartbeat::print_blocker`]); a low
/// one is a warning, because it usually still works for ordinary labels.
fn warn_if_battery_low(power_level: Option<u8>) {
    if let Some(level) = power_level
        && level <= LOW_BATTERY_LEVEL
    {
        warn!(
            level,
            "battery low — dense or dark labels may print only partway. \
             Charge the printer, or use a lower --density, if output is clipped"
        );
    }
}

impl<T: Transport> PrinterClient<T> {
    pub fn new(transport: T, model: Model) -> Self {
        Self {
            transport,
            model,
            task: PrintTask::for_model(model),
            pacing: Pacing::REAL,
            decoder: PacketDecoder::new(),
        }
    }

    /// Override the print-task sequence (default comes from [`PrintTask::for_model`]).
    pub fn with_print_task(mut self, task: PrintTask) -> Self {
        self.task = task;
        self
    }

    /// Override timings and retry budget (see [`Pacing::INSTANT`] for tests).
    pub fn with_pacing(mut self, pacing: Pacing) -> Self {
        self.pacing = pacing;
        self
    }

    /// Widest raster this client will accept, given both model and print task.
    pub fn max_width_px(&self) -> u32 {
        effective_max_width_px(self.model, self.task)
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn print_task(&self) -> PrintTask {
        self.task
    }

    /// Access individual protocol operations for diagnostics and experiments.
    pub fn raw(&mut self) -> RawPrinter<'_, T> {
        RawPrinter { client: self }
    }

    /// Map a protocol ACK bool into a hard error when the printer rejected the step.
    pub(crate) fn require_ack(ok: bool, step: &'static str, cmd: u8) -> Result<()> {
        if ok {
            Ok(())
        } else {
            Err(Error::CommandRejected { step, cmd })
        }
    }

    pub(crate) async fn maybe_sleep(&self, d: Duration) {
        if !d.is_zero() {
            tokio::time::sleep(d).await;
        }
    }

    /// Heartbeat preflight: abort on open cover, missing paper, or empty battery.
    ///
    /// If heartbeat cannot be read, printing continues (firmware-dependent; doctor is
    /// the place for full diagnostics).
    pub async fn preflight_ready(&mut self) -> Result<()> {
        match self.heartbeat().await {
            Ok(hb) => {
                info!(
                    power = ?hb.power_level,
                    lid = ?hb.closing_state,
                    paper = ?hb.paper_state,
                    rfid = ?hb.rfid_read_state,
                    "preflight heartbeat"
                );
                if let Some(code) = hb.print_blocker() {
                    return Err(Error::Printer(code));
                }
                warn_if_battery_low(hb.power_level);
                Ok(())
            }
            Err(e) => {
                warn!(error = %e, "preflight heartbeat unavailable; continuing");
                Ok(())
            }
        }
    }

    pub async fn close(mut self) -> Result<()> {
        self.transport.close().await
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
