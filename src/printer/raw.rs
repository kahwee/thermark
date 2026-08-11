//! Explicit advanced access to individual protocol commands.
//!
//! Normal callers should use [`super::PrinterClient::print_raster`] and the
//! query helpers. This API exists for protocol experiments and deliberately
//! makes the escape hatch visible at the call site: `client.raw().…`.

use super::client::PrinterClient;
use crate::errors::{Error, PrinterFault, Result};
use crate::packet::Packet;
use crate::protocol::Cmd;
use crate::transport::Transport;
use crate::types::Density;
use std::num::NonZeroU32;
use std::time::Duration;
use tracing::debug;

impl<T: Transport> PrinterClient<T> {
    pub(crate) async fn send_pkt(&mut self, packet: &Packet) -> Result<()> {
        self.transport.send_packet(packet).await
    }

    pub(crate) async fn recv_pkts(&mut self, wait: Duration) -> Result<Vec<Packet>> {
        let bytes = self.transport.recv_raw(wait).await?;
        let packets = self.decoder.push(&bytes);
        for packet in &packets {
            debug!(
                cmd = format_args!("{:#04x}", packet.cmd),
                data = %hex::encode(&packet.data),
                "RX pkt"
            );
        }
        Ok(packets)
    }

    /// Send a request and wait for a matching response command.
    ///
    /// Uses [`OnTimeout::WaitOnly`] — safe for any command. Prefer
    /// [`Self::transceive_with`] and [`OnTimeout::Resend`] for reads.
    pub(crate) async fn transceive(
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
    pub(crate) async fn transceive_with(
        &mut self,
        request: Packet,
        response_cmd: u8,
        attempts: u32,
        wait: Duration,
        on_timeout: OnTimeout,
    ) -> Result<Packet> {
        if attempts == 0 {
            return Err(Error::InvalidRetryBudget);
        }
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
                    return Err(Error::Printer(PrinterFault::from_u8(code)));
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

    /// Response offset style: reply command = request command + offset.
    pub(crate) async fn transceive_offset(
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

    pub(crate) async fn set_label_type(&mut self, t: u8) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::SetLabelType as u8, vec![t], 0x10, OnTimeout::Resend)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub(crate) async fn set_density(&mut self, level: Density) -> Result<bool> {
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

    pub(crate) async fn start_print(&mut self) -> Result<bool> {
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

    pub(crate) async fn end_print(&mut self) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::PrintEnd as u8, vec![0x01], 1, OnTimeout::WaitOnly)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub(crate) async fn start_page(&mut self) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::PageStart as u8, vec![0x01], 1, OnTimeout::WaitOnly)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub(crate) async fn end_page(&mut self) -> Result<bool> {
        let pkt = self
            .transceive_offset(Cmd::PageEnd as u8, vec![0x01], 1, OnTimeout::WaitOnly)
            .await?;
        Ok(pkt.data.first().copied().unwrap_or(0) != 0)
    }

    pub(crate) async fn set_page_size(&mut self, rows: u16, cols: u16) -> Result<bool> {
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

    /// Fire-and-forget send (used for image row stream).
    pub(crate) async fn send_raw_packet(&mut self, packet: Packet) -> Result<()> {
        self.send_pkt(&packet).await
    }
}

/// What to do when a raw request gets no reply within the wait window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnTimeout {
    /// Keep waiting; send the request exactly once.
    #[default]
    WaitOnly,
    /// Resend the request on each attempt; only safe for idempotent commands.
    Resend,
}

/// Explicit access to low-level operations that can violate printer state.
pub struct RawPrinter<'a, T: Transport> {
    pub(crate) client: &'a mut PrinterClient<T>,
}

impl<T: Transport> RawPrinter<'_, T> {
    pub async fn transceive(
        &mut self,
        request: Packet,
        response_cmd: u8,
        attempts: NonZeroU32,
        wait: Duration,
        on_timeout: OnTimeout,
    ) -> Result<Packet> {
        self.client
            .transceive_with(request, response_cmd, attempts.get(), wait, on_timeout)
            .await
    }

    pub async fn send_packet(&mut self, packet: Packet) -> Result<()> {
        self.client.send_raw_packet(packet).await
    }

    pub async fn set_label_type(&mut self, label_type: u8) -> Result<bool> {
        self.client.set_label_type(label_type).await
    }

    pub async fn set_density(&mut self, density: Density) -> Result<bool> {
        self.client.set_density(density).await
    }

    pub async fn start_print(&mut self) -> Result<bool> {
        self.client.start_print().await
    }

    pub async fn end_print(&mut self) -> Result<bool> {
        self.client.end_print().await
    }

    pub async fn start_page(&mut self) -> Result<bool> {
        self.client.start_page().await
    }

    pub async fn end_page(&mut self) -> Result<bool> {
        self.client.end_page().await
    }

    pub async fn set_page_size(&mut self, rows: u16, columns: u16) -> Result<bool> {
        self.client.set_page_size(rows, columns).await
    }
}
