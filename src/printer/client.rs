//! High-level printer client (protocol state machine / print jobs).

use super::info::{Heartbeat, InfoValue, PrinterSummary, RfidInfo};
use crate::errors::{Error, PrinterErrorCode, Result};
use crate::geometry::{LabelMm, LabelPx};
use crate::image_encode;
use crate::packet::Packet;
use crate::print_task::PrintTask;
use crate::protocol::{self, Cmd, InfoKey, Model};
use crate::transport::Transport;
use crate::types::{Density, Rotation, Threshold};
use std::path::Path;
use std::time::Duration;
use tracing::{debug, info, warn};

pub struct PrinterClient<T: Transport> {
    transport: T,
    model: Model,
    task: PrintTask,
    /// When false, skip pacing sleeps (for unit tests).
    pace: bool,
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
    /// If true with `label`, scale image to **cover** the label (max size).
    pub fill: bool,
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
        }
    }
}

impl<T: Transport> PrinterClient<T> {
    pub fn new(transport: T, model: Model) -> Self {
        Self {
            transport,
            model,
            task: PrintTask::for_model(model),
            pace: true,
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

    /// Disable inter-packet sleeps (unit tests).
    pub fn with_pace(mut self, pace: bool) -> Self {
        self.pace = pace;
        self
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

    async fn maybe_sleep(&self, d: Duration) {
        if self.pace && !d.is_zero() {
            tokio::time::sleep(d).await;
        }
    }

    async fn send_pkt(&mut self, packet: &Packet) -> Result<()> {
        self.transport.send_packet(packet).await
    }

    async fn recv_pkts(&mut self, wait: Duration) -> Result<Vec<Packet>> {
        self.transport.recv_packets(wait).await
    }

    /// Send a request and wait for a matching response command.
    pub async fn transceive(
        &mut self,
        request: Packet,
        response_cmd: u8,
        attempts: u32,
        wait: Duration,
    ) -> Result<Packet> {
        let req_cmd = request.cmd;
        self.send_pkt(&request).await?;
        for i in 0..attempts {
            let packets = self.recv_pkts(wait).await?;
            for p in packets {
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
            if i + 1 < attempts {
                debug!(
                    expected = format_args!("{response_cmd:#04x}"),
                    "retry wait for response"
                );
            }
        }
        Err(Error::Timeout {
            expected: response_cmd,
            request: req_cmd,
        })
    }

    /// Response offset style used by the simple print-task form (resp = req + offset).
    async fn transceive_offset(&mut self, cmd: u8, data: Vec<u8>, offset: u8) -> Result<Packet> {
        let req = Packet::new(cmd, data);
        let resp = cmd.wrapping_add(offset);
        self.transceive(req, resp, 8, Duration::from_millis(150))
            .await
    }

    pub async fn get_info(&mut self, key: InfoKey) -> Result<InfoValue> {
        let req = protocol::info(key);
        let resp_cmd = (Cmd::PrinterInfo as u8).wrapping_add(key as u8);
        let pkt = self
            .transceive(req, resp_cmd, 8, Duration::from_millis(200))
            .await?;
        Ok(InfoValue::parse(key, &pkt.data))
    }

    pub async fn heartbeat(&mut self) -> Result<Heartbeat> {
        if let Ok(pkt) = self
            .transceive_offset(Cmd::Heartbeat as u8, vec![0x01], 1)
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
            .transceive_offset(Cmd::SetLabelType as u8, vec![t], 0x10)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn set_density(&mut self, level: Density) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::SetDensity as u8, vec![level.get()], 0x10)
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
            .transceive_offset(Cmd::PrintEnd as u8, vec![0x01], 1)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn start_page(&mut self) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::PageStart as u8, vec![0x01], 1)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn end_page(&mut self) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::PageEnd as u8, vec![0x01], 1)
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
    pub async fn print_rows(
        &mut self,
        width: u32,
        height: u32,
        rows: Vec<Packet>,
        density: Density,
    ) -> Result<()> {
        info!(
            width,
            height,
            task = %self.task,
            density = density.get(),
            rows = rows.len(),
            "print job"
        );

        let rows_u16 = u16::try_from(height).map_err(|_| Error::ImageTooLarge { width, height })?;
        let cols_u16 = u16::try_from(width).map_err(|_| Error::ImageTooLarge { width, height })?;
        let max_w = self.model.max_width_px().min(self.task.max_width_px());
        if width > max_w {
            return Err(Error::ImageTooWide { width, max: max_w });
        }

        if self.task.uses_print_clear() {
            let _ = self
                .transceive(
                    protocol::pkt(Cmd::PrintClear, vec![0x01]),
                    0x30,
                    4,
                    Duration::from_millis(100),
                )
                .await;
        }

        self.set_density(density).await?;
        self.set_label_type(1).await?;
        self.start_print().await?;
        self.start_page().await?;
        self.set_page_size(rows_u16, cols_u16).await?;

        for (i, pkt) in rows.into_iter().enumerate() {
            self.send_raw_packet(pkt).await?;
            if i % 8 == 7 {
                self.maybe_sleep(Duration::from_millis(8)).await;
            }
        }

        self.end_page().await?;
        self.maybe_sleep(Duration::from_millis(200)).await;

        let polls = if self.pace {
            self.task.status_polls()
        } else {
            1
        };
        for _ in 0..polls {
            let _ = self
                .transceive(
                    protocol::print_status(),
                    0xb3,
                    1,
                    Duration::from_millis(if self.pace { 100 } else { 1 }),
                )
                .await;
            self.maybe_sleep(Duration::from_millis(50)).await;
        }

        let end_tries = if self.pace { 50 } else { 5 };
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
            self.maybe_sleep(Duration::from_millis(100)).await;
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
            },
        )
        .await
    }

    pub async fn print_image_file_opts(&mut self, path: &Path, opts: PrintOptions) -> Result<()> {
        let max_w = self.model.max_width_px();
        let mut img = image::open(path)?;

        if !opts.rotate.is_identity() {
            img = match opts.rotate {
                Rotation::Deg0 => img,
                Rotation::Deg90 => img.rotate90(),
                Rotation::Deg180 => img.rotate180(),
                Rotation::Deg270 => img.rotate270(),
            };
        }

        let label_px: Option<LabelPx> = opts.label.map(|l| l.to_pixels(max_w));
        if let Some(lp) = label_px {
            info!(
                width_px = lp.width_px,
                height_px = lp.height_px,
                width_mm = lp.mm().width_mm,
                height_mm = lp.mm().height_mm,
                max_w,
                "label canvas"
            );
            img = if opts.fill {
                image_encode::fill_label(img, lp)
            } else {
                image_encode::pad_to_label(image_encode::fit_width(img, lp.width_px), lp)
            };
        } else if opts.fit {
            img = image_encode::fit_width(img, max_w);
        }

        let (width, height, rows) =
            image_encode::encode_image(img, max_w, 0, opts.threshold.get())?;

        if let Ok(rfid) = self.rfid_info().await {
            info!(%rfid, "RFID");
        }
        if let Ok(hb) = self.heartbeat().await {
            info!(
                power = ?hb.power_level,
                lid = ?hb.closing_state,
                paper = ?hb.paper_state,
                rfid = ?hb.rfid_read_state,
                "preflight heartbeat"
            );
            if hb.paper_state == Some(1) {
                warn!(
                    "printer reports paper_state=1 (often means no label detected). \
                     Load labels with 2–5mm protruding and close the cover."
                );
            }
        }

        self.print_rows(width, height, rows, opts.density).await
    }

    /// Print an in-memory grayscale image (dark pixels print).
    pub async fn print_gray_image(
        &mut self,
        gray: &image::GrayImage,
        density: Density,
    ) -> Result<()> {
        let max_w = self.model.max_width_px().min(self.task.max_width_px());
        let img = image::DynamicImage::ImageLuma8(gray.clone());
        let (width, height, rows) =
            image_encode::encode_image(img, max_w, 0, Threshold::DEFAULT.get())?;
        self.print_rows(width, height, rows, density).await
    }

    pub async fn rfid_info(&mut self) -> Result<RfidInfo> {
        let pkt = self
            .transceive(protocol::rfid(), 0x1b, 8, Duration::from_millis(250))
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
        PrinterClient::new(MockTransport::new(), Model::B1).with_pace(false)
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
        let mut c = PrinterClient::new(mock, Model::B1).with_pace(false);
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
        let mut c = PrinterClient::new(mock, Model::B1).with_pace(false);
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
            .with_pace(false);
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
        let mut c = PrinterClient::new(mock, Model::B1).with_pace(false);
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
    async fn image_too_large_for_u16_page_size() {
        let mut c = client_b1();
        // Construct rows for absurd height via print_rows directly
        let err = c
            .print_rows(8, u32::from(u16::MAX) + 1, vec![], Density::NORMAL)
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
