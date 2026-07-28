//! High-level printer client (protocol state machine).

use crate::errors::{Error, PrinterErrorCode, Result};
use crate::geometry::{LabelMm, LabelPx};
use crate::image_encode;
use crate::packet::Packet;
use crate::print_task::PrintTask;
use crate::protocol::{self, Cmd, InfoKey, Model};
use crate::transport::Transport;
use tracing::{debug, info, warn};
use std::path::Path;
use std::time::Duration;

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
    pub density: u8,
    pub rotate: u32,
    pub threshold: u8,
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
            density: 3,
            rotate: 0,
            threshold: 127,
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
                debug!(expected = format_args!("{response_cmd:#04x}"), "retry wait for response");
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
    ) -> Result<Packet> {
        let req = Packet::new(cmd, data);
        let resp = cmd.wrapping_add(offset);
        self.transceive(req, resp, 8, Duration::from_millis(150))
            .await
    }

    pub async fn get_info(&mut self, key: InfoKey) -> Result<InfoValue> {
        // Response cmd = 0x40 + key (e.g. serial key 0x0B → 0x4B).
        let req = protocol::info(key);
        let resp_cmd = (Cmd::PrinterInfo as u8).wrapping_add(key as u8);
        let pkt = self
            .transceive(req, resp_cmd, 8, Duration::from_millis(200))
            .await?;
        Ok(InfoValue::parse(key, &pkt.data))
    }

    pub async fn heartbeat(&mut self) -> Result<Heartbeat> {
        // Primary: response cmd = 0xDD (0xDC + 1)
        if let Ok(pkt) = self
            .transceive_offset(Cmd::Heartbeat as u8, vec![0x01], 1)
            .await
        {
            return Ok(Heartbeat::parse(&pkt.data));
        }

        // Some firmwares reply with 0xDE / 0xDF / 0xD9 instead
        self.send_pkt(&protocol::heartbeat()).await?;
        let packets = self.recv_pkts(Duration::from_millis(500)).await?;
        let pkt = packets
            .into_iter()
            .find(|p| matches!(p.cmd, 0xdd | 0xde | 0xdf | 0xd9))
            .ok_or_else(|| Error::msg("no heartbeat response"))?;
        Ok(Heartbeat::parse(&pkt.data))
    }

    pub async fn set_label_type(&mut self, t: u8) -> Result<bool> {
        // Response is 0x33 = 0x23 + 0x10
        let pkt = self
            .transceive_offset(Cmd::SetLabelType as u8, vec![t], 0x10)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn set_density(&mut self, level: u8) -> Result<bool> {
        if !(1..=5).contains(&level) {
            return Err(Error::InvalidDensity(level));
        }
        let pkt = self
            .transceive_offset(Cmd::SetDensity as u8, vec![level], 0x10)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub async fn start_print(&mut self) -> Result<bool> {
        let req = self.task.print_start(1);
        let pkt = self
            .transceive(req, Cmd::PrintStart as u8 + 1, 6, Duration::from_millis(250))
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
            .transceive(req, Cmd::SetPageSize as u8 + 1, 6, Duration::from_millis(200))
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    /// Core raster job after pixels are already row packets.
    pub async fn print_rows(
        &mut self,
        width: u32,
        height: u32,
        rows: Vec<Packet>,
        density: u8,
    ) -> Result<()> {
        info!(
            width,
            height,
            task = %self.task,
            density,
            rows = rows.len(),
            "print job"
        );

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
        self.set_page_size(height as u16, width as u16).await?;

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
            if self.end_print().await.unwrap_or(false) {
                info!("print finished");
                return Ok(());
            }
            self.maybe_sleep(Duration::from_millis(100)).await;
        }
        warn!("end_print did not confirm success; job may still have printed");
        Ok(())
    }

    /// Fire-and-forget send (used for image row stream).
    pub async fn send_raw_packet(&mut self, packet: Packet) -> Result<()> {
        self.send_pkt(&packet).await
    }

    /// Full print job for an image file (B1 print-task sequence from community wiki).
    pub async fn print_image_file(
        &mut self,
        path: &Path,
        density: u8,
        rotate: u32,
        threshold: u8,
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

        if !opts.rotate.is_multiple_of(360) {
            img = match opts.rotate % 360 {
                90 => img.rotate90(),
                180 => img.rotate180(),
                270 => img.rotate270(),
                other => return Err(Error::InvalidRotation(other)),
            };
        }

        // Map physical label → exact pixel canvas
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

        // Always pad width up to a multiple of 8; if still under printhead and
        // no explicit label, leave as-is (the simple print-task form behaviour).
        let (width, height, rows) =
            image_encode::encode_image(img, max_w, 0, opts.threshold)?;

        // Preflight (best-effort)
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
        density: u8,
    ) -> Result<()> {
        let max_w = self.model.max_width_px().min(self.task.max_width_px());
        let img = image::DynamicImage::ImageLuma8(gray.clone());
        let (width, height, rows) = image_encode::encode_image(img, max_w, 0, 127)?;
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

#[derive(Debug, Clone, Default)]
pub struct RfidInfo {
    pub tag_present: bool,
    pub uuid_hex: String,
    pub barcode: String,
    pub serial: String,
    pub all_paper: i16,
    pub used_paper: i16,
    pub consumables_type: u8,
    pub capacity: Option<i16>,
}

impl RfidInfo {
    fn parse(data: &[u8]) -> Self {
        if data.len() <= 1 {
            return Self {
                tag_present: false,
                ..Default::default()
            };
        }
        let mut i = 0usize;
        let mut out = Self {
            tag_present: true,
            ..Default::default()
        };
        if data.len() >= 8 {
            out.uuid_hex = hex::encode(&data[0..8]);
            i = 8;
        }
        // length-prefixed strings
        if i < data.len() {
            let n = data[i] as usize;
            i += 1;
            if i + n <= data.len() {
                out.barcode = String::from_utf8_lossy(&data[i..i + n]).into_owned();
                i += n;
            }
        }
        if i < data.len() {
            let n = data[i] as usize;
            i += 1;
            if i + n <= data.len() {
                out.serial = String::from_utf8_lossy(&data[i..i + n]).into_owned();
                i += n;
            }
        }
        if i + 4 <= data.len() {
            out.all_paper = i16::from_be_bytes([data[i], data[i + 1]]);
            out.used_paper = i16::from_be_bytes([data[i + 2], data[i + 3]]);
            i += 4;
        }
        if i < data.len() {
            out.consumables_type = data[i];
            i += 1;
        }
        if i + 2 <= data.len() {
            out.capacity = Some(i16::from_be_bytes([data[i], data[i + 1]]));
        }
        out
    }
}

impl std::fmt::Display for RfidInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.tag_present {
            return write!(f, "(no RFID tag)");
        }
        write!(
            f,
            "barcode={} serial={} paper={}/{} type={} uuid={}",
            self.barcode,
            self.serial,
            self.used_paper,
            self.all_paper,
            self.consumables_type,
            self.uuid_hex
        )?;
        if let Some(c) = self.capacity {
            write!(f, " capacity={c}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum InfoValue {
    Int(u64),
    Float(f64),
    Hex(String),
    Raw(Vec<u8>),
}

impl InfoValue {
    fn parse(key: InfoKey, data: &[u8]) -> Self {
        match key {
            InfoKey::DeviceSerial => {
                // Often ASCII (e.g. "DEVICE_SERIAL"); fall back to hex.
                if data.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
                    Self::Hex(String::from_utf8_lossy(data).into_owned())
                } else {
                    Self::Hex(hex::encode(data))
                }
            }
            InfoKey::SoftVersion | InfoKey::HardVersion => {
                let n = be_int(data);
                Self::Float(n as f64 / 100.0)
            }
            _ => Self::Int(be_int(data)),
        }
    }
}

impl std::fmt::Display for InfoValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v:.2}"),
            Self::Hex(s) => write!(f, "{s}"),
            Self::Raw(b) => write!(f, "{}", hex::encode(b)),
        }
    }
}

fn be_int(data: &[u8]) -> u64 {
    data.iter().fold(0u64, |acc, b| (acc << 8) | *b as u64)
}

#[derive(Debug, Clone, Default)]
pub struct Heartbeat {
    pub closing_state: Option<u8>,
    pub power_level: Option<u8>,
    pub paper_state: Option<u8>,
    pub rfid_read_state: Option<u8>,
    pub raw_len: usize,
}

impl Heartbeat {
    fn parse(data: &[u8]) -> Self {
        let mut hb = Self {
            raw_len: data.len(),
            ..Default::default()
        };
        match data.len() {
            20 => {
                hb.paper_state = data.get(18).copied();
                hb.rfid_read_state = data.get(19).copied();
            }
            13 => {
                hb.closing_state = data.get(9).copied();
                hb.power_level = data.get(10).copied();
                hb.paper_state = data.get(11).copied();
                hb.rfid_read_state = data.get(12).copied();
            }
            19 => {
                hb.closing_state = data.get(15).copied();
                hb.power_level = data.get(16).copied();
                hb.paper_state = data.get(17).copied();
                hb.rfid_read_state = data.get(18).copied();
            }
            10 => {
                hb.closing_state = data.get(8).copied();
                hb.power_level = data.get(9).copied();
            }
            9 => {
                hb.closing_state = data.get(8).copied();
            }
            _ => {}
        }
        hb
    }
}

#[derive(Debug, Clone)]
pub struct PrinterSummary {
    pub serial: Option<InfoValue>,
    pub soft: Option<InfoValue>,
    pub hard: Option<InfoValue>,
    pub battery: Option<InfoValue>,
    pub device_type: Option<InfoValue>,
    pub heartbeat: Option<Heartbeat>,
    pub rfid: Option<RfidInfo>,
}

impl std::fmt::Display for PrinterSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Printer info")?;
        if let Some(v) = &self.serial {
            writeln!(f, "  serial:       {v}")?;
        }
        if let Some(v) = &self.device_type {
            writeln!(f, "  device type:  {v}")?;
        }
        if let Some(v) = &self.soft {
            writeln!(f, "  soft version: {v}")?;
        }
        if let Some(v) = &self.hard {
            writeln!(f, "  hard version: {v}")?;
        }
        if let Some(v) = &self.battery {
            writeln!(f, "  battery:      {v}")?;
        }
        if let Some(r) = &self.rfid {
            writeln!(f, "  RFID:         {r}")?;
            // Barcodes often embed size like "50*30" or "H5030"
            if !r.barcode.is_empty() {
                writeln!(
                    f,
                    "  tip: barcode often encodes label size — use --label matching your roll"
                )?;
            }
        }
        if let Some(hb) = &self.heartbeat {
            writeln!(f, "  heartbeat ({} bytes):", hb.raw_len)?;
            if let Some(v) = hb.power_level {
                writeln!(f, "    power:  {v}")?;
            }
            if let Some(v) = hb.closing_state {
                writeln!(f, "    lid:    {v}  (0=closed on most models)")?;
            }
            if let Some(v) = hb.paper_state {
                writeln!(f, "    paper:  {v}  (0=inserted on most models)")?;
            }
            if let Some(v) = hb.rfid_read_state {
                writeln!(f, "    rfid:   {v}  (1=RFID ok)")?;
            }
        }
        writeln!(f, "  geometry:     8 px/mm (~203 dpi), B1 max width 384 px (~48 mm)")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockTransport;
    use crate::print_task::PrintTask;
    use image::{GrayImage, Luma};

    fn client_b1() -> PrinterClient<MockTransport> {
        PrinterClient::new(MockTransport::new(), Model::B1).with_pace(false)
    }

    #[tokio::test]
    async fn b1_print_gray_sends_expected_command_order() {
        let mut c = client_b1();
        assert_eq!(c.print_task(), PrintTask::B1);

        // 16x2 black image → two bitmap rows after invert of dark source
        let gray = GrayImage::from_pixel(16, 2, Luma([0]));
        c.print_gray_image(&gray, 4).await.expect("print");

        let cmds = c.transport().tx_cmds();
        // density, label type, print start, page start, page size, row*, page end, status, print end
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

        // B1 page size is 6 bytes: rows=2, cols=16
        let ps = c.transport().first_tx(0x13).expect("page size pkt");
        assert_eq!(ps.data.len(), 6);
        assert_eq!(u16::from_be_bytes([ps.data[0], ps.data[1]]), 2);
        assert_eq!(u16::from_be_bytes([ps.data[2], ps.data[3]]), 16);

        // B1 print start is 7 bytes
        let st = c.transport().first_tx(0x01).expect("start");
        assert_eq!(st.data.len(), 7);
    }

    #[tokio::test]
    async fn print_start_error_lack_paper() {
        let mut mock = MockTransport::new();
        mock.fail_cmd(0x01, 0x02); // LackPaper
        let mut c = PrinterClient::new(mock, Model::B1).with_pace(false);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        let err = c.print_gray_image(&gray, 3).await.unwrap_err();
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
        let err = c.print_gray_image(&gray, 3).await.unwrap_err();
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
        c.print_gray_image(&gray, 3).await.unwrap();
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
        let mut c = client_b1();
        assert!(c.set_density(0).await.is_err());
        assert!(c.set_density(6).await.is_err());
    }
}
