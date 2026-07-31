//! High-level printer client (protocol state machine / print jobs).

use super::info::{Heartbeat, InfoValue, PrinterSummary, RfidInfo};
use crate::errors::{Error, PrinterErrorCode, Result};
use crate::geometry::{LabelMm, SafeArea};
use crate::image_encode::{self, Raster};
use crate::packet::Packet;
use crate::print_task::{PrintTask, effective_max_width_px};
use crate::protocol::{self, Cmd, InfoKey, Model};
use crate::transport::Transport;
use crate::types::{Density, Rotation, Threshold};
use std::path::Path;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Timing and retry budget for a print job.
///
/// Real jobs and tests run the *same* control flow; only the numbers differ, so
/// a retry path can never be exercised in tests but skipped in production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pacing {
    /// Pause taken once per [`Self::pace_bytes`] of streamed row data.
    ///
    /// Pacing by *rows* under-serves dense pages: a solid page and a mostly
    /// blank one send the same packet count in the same time, but four times
    /// the bytes — and a dense page is also what strains the printer's power
    /// and thermal budget. Pacing by bytes gives each page time proportional to
    /// what it actually asks the printer to do.
    pub row_pause: Duration,
    /// Bytes of row data sent between pauses.
    pub pace_bytes: usize,
    /// Settle time after PageEnd.
    pub after_page_end: Duration,
    /// Pause between status polls.
    pub between_polls: Duration,
    /// Time to wait for each status-poll reply.
    pub poll_wait: Duration,
    /// How many times to retry PrintEnd before giving up.
    pub end_print_tries: u32,
    /// Pause between PrintEnd attempts.
    pub between_end_tries: Duration,
}

impl Default for Pacing {
    fn default() -> Self {
        Self::REAL
    }
}

impl Pacing {
    /// Timings tuned against real B1 hardware.
    pub const REAL: Self = Self {
        // Comparable to the reference implementation, which delays 10ms after
        // every packet (the protocol reference `packetIntervalMs`). Combined with the 5ms
        // per BLE chunk in the transport, a dense page is paced ~2.3s — the
        // old fixed 8ms-per-8-rows gave it ~1.4s regardless of size.
        row_pause: Duration::from_millis(5),
        pace_bytes: 64,
        after_page_end: Duration::from_millis(200),
        between_polls: Duration::from_millis(50),
        poll_wait: Duration::from_millis(100),
        end_print_tries: 50,
        between_end_tries: Duration::from_millis(100),
    };

    /// Six times the row pacing of [`Self::REAL`], via `THERMARK_SLOW=1`.
    ///
    /// Kept as a diagnostic. When a page truncates, this distinguishes "sent
    /// too fast" from everything else — though on the hardware here the cause
    /// turned out to be a low battery rather than pacing, and this only
    /// improved matters because slower printing draws less average current.
    pub const CAREFUL: Self = Self {
        row_pause: Duration::from_millis(32),
        pace_bytes: 256,
        after_page_end: Duration::from_millis(300),
        between_polls: Duration::from_millis(50),
        poll_wait: Duration::from_millis(100),
        end_print_tries: 50,
        between_end_tries: Duration::from_millis(100),
    };

    /// Same sequence and same retry counts, without the waiting — for tests
    /// against [`crate::mock::MockTransport`], which replies instantly.
    pub const INSTANT: Self = Self {
        row_pause: Duration::ZERO,
        pace_bytes: 256,
        after_page_end: Duration::ZERO,
        between_polls: Duration::ZERO,
        poll_wait: Duration::from_millis(1),
        end_print_tries: 50,
        between_end_tries: Duration::ZERO,
    };
}

/// What to do when a request gets no reply within the wait window.
///
/// BLE writes are unacknowledged, so a lost request looks exactly like a slow
/// printer. Waiting longer cannot recover a write that never arrived — only
/// resending can. But a resend is unsafe when the *reply* was the thing lost:
/// the printer already acted, and a second `PrintStart` would start a second
/// job. So this is chosen per command rather than globally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnTimeout {
    /// Keep waiting; send the request exactly once.
    ///
    /// Correct for commands that advance printer state — `PrintStart`,
    /// `PageStart`, `SetPageSize`, `PageEnd`, `PrintEnd`.
    #[default]
    WaitOnly,
    /// Resend the request on each attempt.
    ///
    /// Only for commands where acting twice equals acting once: reads
    /// (`PrinterInfo`, `Heartbeat`, `RfidInfo`, `PrintStatus`) and idempotent
    /// settings (`SetDensity`, `SetLabelType`, `PrintClear`).
    Resend,
}

pub struct PrinterClient<T: Transport> {
    transport: T,
    model: Model,
    task: PrintTask,
    pacing: Pacing,
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
/// Only an empty battery blocks printing ([`Heartbeat::print_blocker`]); a low
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

/// Lay an image out on its label canvas exactly as printing would.
///
/// Split out from the print path so a preview can be produced without a
/// printer — `thermark print --preview out.png` renders through this.
pub fn compose_for_label(
    path: &Path,
    opts: &PrintOptions,
    max_width_px: u32,
) -> Result<image::DynamicImage> {
    let mut img = image_encode::rotate(image::open(path)?, opts.rotate);
    if opts.trim {
        img = image_encode::trim_white(img, opts.threshold.get());
    }

    match opts.label.map(|l| l.to_pixels(max_width_px)) {
        Some(lp) => {
            info!(
                width_px = lp.width_px,
                height_px = lp.height_px,
                width_mm = lp.mm().width_mm,
                height_mm = lp.mm().height_mm,
                max_w = max_width_px,
                fill = opts.fill,
                margin_px = opts.margin_px,
                dither = opts.dither,
                safe_bottom = opts.safe.bottom,
                "label canvas"
            );
            img = if opts.fill {
                image_encode::fill_label(img, lp, opts.safe, opts.margin_px)
            } else {
                // Contain + center: whole photo visible, white margins, no crop.
                image_encode::contain_label(img, lp, opts.safe, opts.margin_px)
            };
        }
        None if opts.fit => img = image_encode::fit_width(img, max_width_px),
        None => {}
    }
    Ok(img)
}

/// Options for a raster print job.
#[derive(Debug, Clone)]
pub struct PrintOptions {
    pub density: Density,
    pub rotate: Rotation,
    pub threshold: Threshold,
    /// Scale down only if wider than printhead.
    pub fit: bool,
    /// Physical label size (mm). Image is scaled/padded to this.
    pub label: Option<LabelMm>,
    /// If true with `label`, scale image to **cover** the label (may crop).
    /// If false, **contain** + center with white margins (better for photos).
    pub fill: bool,
    /// White inset margin in pixels (each side) when placing on a label canvas.
    pub margin_px: u32,
    /// Floyd–Steinberg dither instead of hard B/W threshold (photos).
    pub dither: bool,
    /// Printable insets. Raw images are placed inside this so nothing lands in
    /// the band the printer cannot reach. Use [`SafeArea::NONE`] for content
    /// that already accounts for it (rendered stickers, calibration patterns).
    pub safe: SafeArea,
    /// Crop the source image's own white border before placing it, so the
    /// artwork's margin is not added to the label's.
    pub trim: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            density: Density::NORMAL,
            rotate: Rotation::Deg0,
            threshold: Threshold::DEFAULT,
            fit: false,
            label: None,
            fill: true,
            margin_px: 0,
            dither: false,
            safe: SafeArea::default(),
            trim: true,
        }
    }
}

impl<T: Transport> PrinterClient<T> {
    pub fn new(transport: T, model: Model) -> Self {
        Self {
            transport,
            model,
            task: PrintTask::for_model(model),
            pacing: Pacing::REAL,
        }
    }

    /// Override the print-task sequence (default comes from [`PrintTask::for_model`]).
    pub fn with_print_task(mut self, task: PrintTask) -> Self {
        self.task = task;
        self
    }

    /// Force simple 1-byte PrintStart / 4-byte page size (alias for `PrintTask::Simple`).
    pub fn with_simple_print_start(mut self, yes: bool) -> Self {
        if yes {
            self.task = PrintTask::Simple;
        }
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

    pub fn into_transport(self) -> T {
        self.transport
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn print_task(&self) -> PrintTask {
        self.task
    }

    /// Map a protocol ACK bool into a hard error when the printer rejected the step.
    fn require_ack(ok: bool, step: &'static str, cmd: u8) -> Result<()> {
        if ok {
            Ok(())
        } else {
            Err(Error::CommandRejected { step, cmd })
        }
    }

    async fn maybe_sleep(&self, d: Duration) {
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

    async fn send_pkt(&mut self, packet: &Packet) -> Result<()> {
        self.transport.send_packet(packet).await
    }

    async fn recv_pkts(&mut self, wait: Duration) -> Result<Vec<Packet>> {
        self.transport.recv_packets(wait).await
    }

    /// Send a request and wait for a matching response command.
    ///
    /// Uses [`OnTimeout::WaitOnly`] — safe for any command. Prefer
    /// [`Self::transceive_with`] and [`OnTimeout::Resend`] for reads.
    pub async fn transceive(
        &mut self,
        request: Packet,
        response_cmd: u8,
        attempts: u32,
        wait: Duration,
    ) -> Result<Packet> {
        self.transceive_with(request, response_cmd, attempts, wait, OnTimeout::WaitOnly)
            .await
    }

    /// Send a request and wait for a matching response, with an explicit
    /// policy for what to do when a reply does not arrive in time.
    pub async fn transceive_with(
        &mut self,
        request: Packet,
        response_cmd: u8,
        attempts: u32,
        wait: Duration,
        on_timeout: OnTimeout,
    ) -> Result<Packet> {
        let req_cmd = request.cmd;
        for attempt in 0..attempts {
            // BLE writes go out unacknowledged (`WriteType::WithoutResponse`),
            // so a lost request is indistinguishable from a slow printer and no
            // amount of extra waiting recovers it — only a resend does.
            if attempt == 0 || on_timeout == OnTimeout::Resend {
                if attempt > 0 {
                    debug!(
                        cmd = format_args!("{req_cmd:#04x}"),
                        attempt, "resending request"
                    );
                }
                self.send_pkt(&request).await?;
            }

            for p in self.recv_pkts(wait).await? {
                if p.cmd == 0xdb {
                    let code = p.data.first().copied().unwrap_or(0);
                    return Err(Error::Printer(PrinterErrorCode::from_u8(code)));
                }
                if p.cmd == response_cmd {
                    return Ok(p);
                }
                debug!(
                    cmd = format_args!("{:#04x}", p.cmd),
                    expected = format_args!("{response_cmd:#04x}"),
                    "ignoring pkt while waiting for response"
                );
            }
        }
        Err(Error::Timeout {
            expected: response_cmd,
            request: req_cmd,
        })
    }

    /// Response offset style used by the simple print-task form (resp = req + offset).
    async fn transceive_offset(
        &mut self,
        cmd: u8,
        data: Vec<u8>,
        offset: u8,
        on_timeout: OnTimeout,
    ) -> Result<Packet> {
        let req = Packet::new(cmd, data);
        let resp = cmd.wrapping_add(offset);
        self.transceive_with(req, resp, 8, Duration::from_millis(150), on_timeout)
            .await
    }

    pub async fn get_info(&mut self, key: InfoKey) -> Result<InfoValue> {
        let req = protocol::info(key);
        let resp_cmd = (Cmd::PrinterInfo as u8).wrapping_add(key as u8);
        let pkt = self
            .transceive_with(
                req,
                resp_cmd,
                8,
                Duration::from_millis(200),
                OnTimeout::Resend,
            )
            .await?;
        Ok(InfoValue::parse(key, &pkt.data))
    }

    pub async fn heartbeat(&mut self) -> Result<Heartbeat> {
        if let Ok(pkt) = self
            .transceive_offset(Cmd::Heartbeat as u8, vec![0x01], 1, OnTimeout::Resend)
            .await
        {
            return Ok(Heartbeat::parse(&pkt.data));
        }

        self.send_pkt(&protocol::heartbeat()).await?;
        let packets = self.recv_pkts(Duration::from_millis(500)).await?;
        let pkt = packets
            .into_iter()
            .find(|p| matches!(p.cmd, 0xdd | 0xde | 0xdf | 0xd9))
            .ok_or_else(|| Error::msg("no heartbeat response"))?;
        Ok(Heartbeat::parse(&pkt.data))
    }

    pub async fn set_label_type(&mut self, t: u8) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::SetLabelType as u8, vec![t], 0x10, OnTimeout::Resend)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn set_density(&mut self, level: Density) -> Result<bool> {
        let pkt = self
            .transceive_offset(
                Cmd::SetDensity as u8,
                vec![level.get()],
                0x10,
                OnTimeout::Resend,
            )
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn start_print(&mut self) -> Result<bool> {
        let req = self.task.print_start(1);
        let pkt = self
            .transceive(
                req,
                Cmd::PrintStart as u8 + 1,
                6,
                Duration::from_millis(250),
            )
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn end_print(&mut self) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::PrintEnd as u8, vec![0x01], 1, OnTimeout::WaitOnly)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn start_page(&mut self) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::PageStart as u8, vec![0x01], 1, OnTimeout::WaitOnly)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn end_page(&mut self) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::PageEnd as u8, vec![0x01], 1, OnTimeout::WaitOnly)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn set_page_size(&mut self, rows: u16, cols: u16) -> Result<bool> {
        let req = self.task.set_page_size(rows, cols, 1);
        let pkt = self
            .transceive(
                req,
                Cmd::SetPageSize as u8 + 1,
                6,
                Duration::from_millis(200),
            )
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    /// Core raster job after pixels are already row packets.
    ///
    /// Returns [`Error::PrintNotConfirmed`] if the printer never ACKs PrintEnd.
    pub async fn print_raster(&mut self, raster: Raster, density: Density) -> Result<()> {
        let (width, height) = (raster.width, raster.height);
        info!(
            width,
            height,
            task = %self.task,
            density = density.get(),
            rows = raster.rows.len(),
            "print job"
        );

        let rows_u16 = u16::try_from(height).map_err(|_| Error::ImageTooLarge { width, height })?;
        let cols_u16 = u16::try_from(width).map_err(|_| Error::ImageTooLarge { width, height })?;
        let max_w = self.max_width_px();
        if width > max_w {
            return Err(Error::ImageTooWide { width, max: max_w });
        }

        if self.task.uses_print_clear() {
            // Optional step: a missing reply is fine, but a reported fault is
            // not — only `Error::Printer` aborts.
            if let Err(e @ Error::Printer(_)) = self
                .transceive_with(
                    protocol::pkt(Cmd::PrintClear, vec![0x01]),
                    0x30,
                    4,
                    Duration::from_millis(100),
                    OnTimeout::Resend,
                )
                .await
            {
                return Err(e);
            }
        }

        // Soft NACKs (Ok(false)) are hard failures — do not stream rows after a reject.
        Self::require_ack(
            self.set_density(density).await?,
            "set_density",
            Cmd::SetDensity as u8,
        )?;
        Self::require_ack(
            self.set_label_type(1).await?,
            "set_label_type",
            Cmd::SetLabelType as u8,
        )?;
        Self::require_ack(
            self.start_print().await?,
            "start_print",
            Cmd::PrintStart as u8,
        )?;
        Self::require_ack(self.start_page().await?, "start_page", Cmd::PageStart as u8)?;
        Self::require_ack(
            self.set_page_size(rows_u16, cols_u16).await?,
            "set_page_size",
            Cmd::SetPageSize as u8,
        )?;

        // Pace by bytes, not rows — see `Pacing::row_pause`.
        let mut unpaced_bytes = 0usize;
        for pkt in raster.rows {
            unpaced_bytes += crate::packet::FRAME_OVERHEAD + pkt.data.len();
            self.send_raw_packet(pkt).await?;
            if unpaced_bytes >= self.pacing.pace_bytes {
                unpaced_bytes = 0;
                self.maybe_sleep(self.pacing.row_pause).await;
            }
        }

        Self::require_ack(self.end_page().await?, "end_page", Cmd::PageEnd as u8)?;
        self.maybe_sleep(self.pacing.after_page_end).await;

        for _ in 0..self.task.status_polls() {
            // A missing status reply is normal on some firmware, but if the
            // printer named its fault (cover, paper, …), surface that instead
            // of grinding through end_print retries into PrintNotConfirmed.
            if let Err(e @ Error::Printer(_)) = self
                .transceive_with(
                    protocol::print_status(),
                    0xb3,
                    1,
                    self.pacing.poll_wait,
                    OnTimeout::Resend,
                )
                .await
            {
                return Err(e);
            }
            self.maybe_sleep(self.pacing.between_polls).await;
        }

        let end_tries = self.pacing.end_print_tries;
        for _ in 0..end_tries {
            match self.end_print().await {
                Ok(true) => {
                    info!("print finished");
                    return Ok(());
                }
                Ok(false) => {
                    // Printer replied but did not confirm success — retry.
                }
                Err(Error::Timeout { .. }) => {
                    // No reply yet — retry.
                }
                Err(e) => {
                    // Real protocol / transport / printer error — surface it.
                    return Err(e);
                }
            }
            self.maybe_sleep(self.pacing.between_end_tries).await;
        }
        warn!("end_print did not confirm success after {end_tries} tries");
        Err(Error::PrintNotConfirmed)
    }

    /// Fire-and-forget send (used for image row stream).
    pub async fn send_raw_packet(&mut self, packet: Packet) -> Result<()> {
        self.send_pkt(&packet).await
    }

    /// Full print job for an image file.
    pub async fn print_image_file(
        &mut self,
        path: &Path,
        density: Density,
        rotate: Rotation,
        threshold: Threshold,
        fit: bool,
    ) -> Result<()> {
        self.print_image_file_opts(
            path,
            PrintOptions {
                density,
                rotate,
                threshold,
                fit,
                label: None,
                fill: false,
                margin_px: 0,
                dither: false,
                safe: SafeArea::default(),
                trim: true,
            },
        )
        .await
    }

    pub async fn print_image_file_opts(&mut self, path: &Path, opts: PrintOptions) -> Result<()> {
        // Size the canvas against the same limit the raster is checked against,
        // so a mismatched --model/--task fails before encoding, not after.
        let max_w = self.max_width_px();
        let img = compose_for_label(path, &opts, max_w)?;
        let raster = image_encode::encode(img, max_w, opts.threshold.get(), opts.dither)?;

        if let Ok(rfid) = self.rfid_info().await {
            info!(%rfid, "RFID");
        }
        self.preflight_ready().await?;

        self.print_raster(raster, opts.density).await
    }

    /// Print an in-memory grayscale image (dark pixels print).
    pub async fn print_gray_image(
        &mut self,
        gray: &image::GrayImage,
        density: Density,
    ) -> Result<()> {
        let img = image::DynamicImage::ImageLuma8(gray.clone());
        let raster =
            image_encode::encode(img, self.max_width_px(), Threshold::DEFAULT.get(), false)?;
        self.print_raster(raster, density).await
    }

    pub async fn rfid_info(&mut self) -> Result<RfidInfo> {
        let pkt = self
            .transceive_with(
                protocol::rfid(),
                0x1b,
                8,
                Duration::from_millis(250),
                OnTimeout::Resend,
            )
            .await?;
        Ok(RfidInfo::parse(&pkt.data))
    }

    pub async fn fetch_summary(&mut self) -> Result<PrinterSummary> {
        let serial = self.get_info(InfoKey::DeviceSerial).await.ok();
        let soft = self.get_info(InfoKey::SoftVersion).await.ok();
        let hard = self.get_info(InfoKey::HardVersion).await.ok();
        let battery = self.get_info(InfoKey::Battery).await.ok();
        let device_type = self.get_info(InfoKey::DeviceType).await.ok();
        let hb = self.heartbeat().await.ok();
        let rfid = self.rfid_info().await.ok();
        Ok(PrinterSummary {
            serial,
            soft,
            hard,
            battery,
            device_type,
            heartbeat: hb,
            rfid,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockTransport;
    use crate::print_task::PrintTask;
    use crate::types::Density;
    use image::{GrayImage, Luma};

    fn client_b1() -> PrinterClient<MockTransport> {
        PrinterClient::new(MockTransport::new(), Model::B1).with_pacing(Pacing::INSTANT)
    }

    #[tokio::test]
    async fn b1_print_gray_sends_expected_command_order() {
        let mut c = client_b1();
        assert_eq!(c.print_task(), PrintTask::B1);

        let gray = GrayImage::from_pixel(16, 2, Luma([0]));
        c.print_gray_image(&gray, Density::DARK)
            .await
            .expect("print");

        let cmds = c.transport().tx_cmds();
        assert!(cmds.contains(&0x21), "density: {cmds:?}");
        assert!(cmds.contains(&0x23), "label type: {cmds:?}");
        assert!(cmds.contains(&0x01), "print start: {cmds:?}");
        assert!(cmds.contains(&0x03), "page start: {cmds:?}");
        assert!(cmds.contains(&0x13), "page size: {cmds:?}");
        assert!(
            cmds.iter().any(|c| *c == 0x85 || *c == 0x84),
            "row data: {cmds:?}"
        );
        assert!(cmds.contains(&0xe3), "page end: {cmds:?}");
        assert!(cmds.contains(&0xa3), "status: {cmds:?}");
        assert!(cmds.contains(&0xf3), "print end: {cmds:?}");

        let ps = c.transport().first_tx(0x13).expect("page size pkt");
        assert_eq!(ps.data.len(), 6);
        assert_eq!(u16::from_be_bytes([ps.data[0], ps.data[1]]), 2);
        assert_eq!(u16::from_be_bytes([ps.data[2], ps.data[3]]), 16);

        let st = c.transport().first_tx(0x01).expect("start");
        assert_eq!(st.data.len(), 7);
    }

    #[tokio::test]
    async fn print_start_error_lack_paper() {
        let mut mock = MockTransport::new();
        mock.fail_cmd(0x01, 0x02);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        let err = c
            .print_gray_image(&gray, Density::NORMAL)
            .await
            .unwrap_err();
        match err {
            Error::Printer(PrinterErrorCode::LackPaper) => {}
            other => panic!("expected LackPaper, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn print_start_error_cover_open() {
        let mut mock = MockTransport::new();
        mock.fail_cmd(0x01, 0x01);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        let err = c
            .print_gray_image(&gray, Density::NORMAL)
            .await
            .unwrap_err();
        match err {
            Error::Printer(PrinterErrorCode::CoverOpen) => {}
            other => panic!("expected CoverOpen, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn simple_task_uses_short_print_start() {
        let mut c = PrinterClient::new(MockTransport::new(), Model::B1)
            .with_print_task(PrintTask::Simple)
            .with_pacing(Pacing::INSTANT);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        c.print_gray_image(&gray, Density::NORMAL).await.unwrap();
        let st = c.transport().first_tx(0x01).unwrap();
        assert_eq!(st.data, vec![0x01]);
        let ps = c.transport().first_tx(0x13).unwrap();
        assert_eq!(ps.data.len(), 4);
    }

    #[tokio::test]
    async fn fetch_summary_reads_info_keys() {
        let mut c = client_b1();
        let s = c.fetch_summary().await.unwrap();
        assert!(s.serial.is_some());
        assert!(s.heartbeat.is_some());
        let cmds = c.transport().tx_cmds();
        assert!(cmds.contains(&0x40));
        assert!(cmds.contains(&0xdc));
    }

    #[tokio::test]
    async fn density_out_of_range_errors() {
        assert!(Density::new(0).is_err());
        assert!(Density::new(6).is_err());
        let mut c = client_b1();
        assert!(c.set_density(Density::NORMAL).await.is_ok());
    }

    #[tokio::test]
    async fn print_not_confirmed_when_end_print_muted() {
        let mut mock = MockTransport::new();
        mock.mute_cmd(0xf3); // no PrintEnd reply
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        let err = c
            .print_gray_image(&gray, Density::NORMAL)
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::PrintNotConfirmed),
            "expected PrintNotConfirmed, got {err:?}"
        );
    }

    #[tokio::test]
    async fn density_nack_is_hard_error() {
        let mut mock = MockTransport::new();
        mock.reject_cmd(0x21);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        let err = c
            .print_gray_image(&gray, Density::NORMAL)
            .await
            .unwrap_err();
        match err {
            Error::CommandRejected { step, cmd } => {
                assert_eq!(step, "set_density");
                assert_eq!(cmd, 0x21);
            }
            other => panic!("expected CommandRejected, got {other:?}"),
        }
        // Must not have started streaming rows after density NACK.
        let cmds = c.transport().tx_cmds();
        assert!(!cmds.iter().any(|c| *c == 0x85 || *c == 0x84));
    }

    #[tokio::test]
    async fn start_print_nack_is_hard_error() {
        let mut mock = MockTransport::new();
        mock.reject_cmd(0x01);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        let err = c
            .print_gray_image(&gray, Density::NORMAL)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::CommandRejected {
                    step: "start_print",
                    cmd: 0x01
                }
            ),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn preflight_blocks_open_cover() {
        let mut mock = MockTransport::new();
        mock.heartbeat_not_ready_cover_open();
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let err = c.preflight_ready().await.unwrap_err();
        assert!(
            matches!(err, Error::Printer(PrinterErrorCode::CoverOpen)),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn low_battery_warns_but_does_not_block() {
        // Level 1 is "low": dense pages may truncate, but ordinary labels
        // usually still print, so this must stay a warning.
        let mut mock = MockTransport::new();
        let mut d = [0u8; 13];
        d[9] = 0; // cover closed
        d[10] = LOW_BATTERY_LEVEL; // battery low
        d[11] = 0; // paper present
        d[12] = 1;
        mock.set_heartbeat(d);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        assert!(c.preflight_ready().await.is_ok());
    }

    #[tokio::test]
    async fn empty_battery_still_blocks() {
        let mut mock = MockTransport::new();
        let mut d = [0u8; 13];
        d[9] = 0;
        d[10] = 0; // empty
        d[11] = 0;
        d[12] = 1;
        mock.set_heartbeat(d);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        assert!(c.preflight_ready().await.is_err());
    }

    #[tokio::test]
    async fn preflight_blocks_no_paper() {
        let mut mock = MockTransport::new();
        mock.heartbeat_not_ready_no_paper();
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let err = c.preflight_ready().await.unwrap_err();
        assert!(
            matches!(err, Error::Printer(PrinterErrorCode::LackPaper)),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn print_image_file_opts_aborts_preflight() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dot.png");
        GrayImage::from_pixel(8, 8, Luma([0])).save(&path).unwrap();

        let mut mock = MockTransport::new();
        mock.heartbeat_not_ready_no_paper();
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let err = c
            .print_image_file_opts(
                &path,
                PrintOptions {
                    density: Density::NORMAL,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::Printer(PrinterErrorCode::LackPaper)),
            "got {err:?}"
        );
        // Must not have entered the print sequence.
        let cmds = c.transport().tx_cmds();
        assert!(
            !cmds.contains(&0x01),
            "print start should not run: {cmds:?}"
        );
    }

    #[tokio::test]
    async fn lost_write_is_recovered_by_resending_a_read() {
        // BLE writes are unacknowledged, so a dropped request can only be
        // recovered by sending it again — waiting longer never helps.
        let mut mock = MockTransport::new();
        mock.drop_first_writes(0x40, 2);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

        let info = c
            .get_info(InfoKey::DeviceSerial)
            .await
            .expect("resend should recover the lost writes");
        assert_eq!(info.to_string(), "TESTMOCK01");

        let sends = c
            .transport()
            .tx_cmds()
            .iter()
            .filter(|c| **c == 0x40)
            .count();
        assert_eq!(sends, 3, "two dropped writes plus the one that landed");
    }

    #[tokio::test]
    async fn state_advancing_commands_are_never_resent() {
        // Resending PrintStart after a lost *reply* would start a second job,
        // so it must go out exactly once no matter how long the reply takes.
        let mut mock = MockTransport::new();
        mock.mute_cmd(0x01);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

        let err = c.start_print().await.unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "got {err:?}");

        let sends = c
            .transport()
            .tx_cmds()
            .iter()
            .filter(|c| **c == 0x01)
            .count();
        assert_eq!(sends, 1, "PrintStart must not be retransmitted");
    }

    #[tokio::test]
    async fn idempotent_settings_are_resent() {
        // SetDensity twice equals SetDensity once, so recovery is safe.
        let mut mock = MockTransport::new();
        mock.drop_first_writes(0x21, 1);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

        assert!(c.set_density(Density::DARK).await.unwrap());
        let sends = c
            .transport()
            .tx_cmds()
            .iter()
            .filter(|c| **c == 0x21)
            .count();
        assert_eq!(sends, 2);
    }

    #[tokio::test]
    async fn mid_job_printer_error_surfaces_instead_of_print_not_confirmed() {
        // The printer reports "out of paper" via 0xDB on the status poll. That
        // result used to be dropped with `let _ =`, so the user got the useless
        // PrintNotConfirmed after 50 pointless end_print retries.
        let mut mock = MockTransport::new();
        mock.fail_cmd(0xa3, 0x02);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        let err = c
            .print_gray_image(&gray, Density::NORMAL)
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::Printer(PrinterErrorCode::LackPaper)),
            "expected the printer's own reason, got {err:?}"
        );
        // And it stopped there rather than pressing on to PrintEnd.
        assert!(!c.transport().tx_cmds().contains(&0xf3));
    }

    #[tokio::test]
    async fn missing_status_reply_still_completes() {
        // A silent status poll is normal on some firmware — it must not abort.
        let mut mock = MockTransport::new();
        mock.mute_cmd(0xa3);
        let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        c.print_gray_image(&gray, Density::NORMAL)
            .await
            .expect("print should finish without a status reply");
    }

    #[tokio::test]
    async fn image_too_large_for_u16_page_size() {
        let mut c = client_b1();
        // Construct rows for absurd height via print_rows directly
        let err = c
            .print_raster(
                Raster {
                    width: 8,
                    height: u32::from(u16::MAX) + 1,
                    rows: vec![],
                },
                Density::NORMAL,
            )
            .await
            .unwrap_err();
        match err {
            Error::ImageTooLarge { height, .. } => {
                assert!(height > u32::from(u16::MAX));
            }
            other => panic!("expected ImageTooLarge, got {other:?}"),
        }
    }
}
