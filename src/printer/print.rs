//! Safe print-job orchestration.

use super::client::PrinterClient;
use super::info::PrintStatus;
use super::job::{PrintOptions, compose_for_label};
use super::raw::OnTimeout;
use crate::errors::{Error, PrinterFault, Result};
use crate::geometry::SafeArea;
use crate::image_encode::{self, Raster};
use crate::packet::FRAME_OVERHEAD;
use crate::print_task::Completion;
use crate::protocol::{self, Cmd};
use crate::transport::Transport;
use crate::types::{Density, Rotation, Threshold};
use std::path::Path;
use std::time::Duration;
use tracing::{debug, info, warn};

impl<T: Transport> PrinterClient<T> {
    /// Print a validated raster using the complete task sequence.
    pub async fn print_raster(&mut self, raster: Raster, density: Density) -> Result<()> {
        let (width, height) = (raster.width(), raster.height());
        let task = self.print_task();
        let profile = self.profile();
        info!(
            width,
            height,
            task = %task,
            density = density.get(),
            row_packets = raster.rows().len(),
            "print job"
        );

        let rows = u16::try_from(height).map_err(|_| Error::ImageTooLarge { width, height })?;
        let columns = u16::try_from(width).map_err(|_| Error::ImageTooLarge { width, height })?;
        raster.validate()?;
        let (_, _, packets) = raster.into_parts();
        let max_width = self.max_width_px();
        if width > max_width {
            return Err(Error::ImageTooWide {
                width,
                max: max_width,
            });
        }

        if !profile.supports_density(density.get()) {
            return Err(Error::msg(format!(
                "density {} is outside the {}..={} range for {}",
                density.get(),
                profile.density_min,
                profile.density_max,
                profile.model
            )));
        }

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

        if task.uses_print_clear() {
            let packet = self
                .transceive_with(
                    protocol::pkt(Cmd::PrintClear, vec![0x01]),
                    0x30,
                    4,
                    Duration::from_millis(100),
                    OnTimeout::Resend,
                )
                .await?;
            Self::require_ack(
                packet.data.first().copied().unwrap_or(0) != 0,
                "print_clear",
                Cmd::PrintClear as u8,
            )?;
        }
        if task.uses_page_start() {
            Self::require_ack(self.start_page().await?, "start_page", Cmd::PageStart as u8)?;
        }
        if task.pre_page_status() {
            self.send_raw_packet(protocol::print_status()).await?;
            self.maybe_sleep(Duration::from_millis(30)).await;
        }
        Self::require_ack(
            self.set_page_size(rows, columns).await?,
            "set_page_size",
            Cmd::SetPageSize as u8,
        )?;
        if task.uses_print_quantity() {
            let packet = self
                .transceive_with(
                    protocol::pkt(Cmd::PrintQuantity, 1u16.to_be_bytes().to_vec()),
                    0x16,
                    6,
                    Duration::from_millis(200),
                    OnTimeout::Resend,
                )
                .await?;
            Self::require_ack(
                packet.data.first().copied().unwrap_or(0) != 0,
                "set_quantity",
                Cmd::PrintQuantity as u8,
            )?;
        }

        let mut unpaced_bytes = 0usize;
        for packet in packets {
            unpaced_bytes += FRAME_OVERHEAD + packet.data.len();
            self.send_raw_packet(packet).await?;
            if unpaced_bytes >= self.pacing.pace_bytes().get() {
                unpaced_bytes = 0;
                self.maybe_sleep(self.pacing.row_pause()).await;
            }
        }

        Self::require_ack(self.end_page().await?, "end_page", Cmd::PageEnd as u8)?;
        self.maybe_sleep(self.pacing.after_page_end()).await;

        self.wait_for_completion(task.completion()).await?;

        let attempts = self.pacing.end_print_tries().get();
        for _ in 0..attempts {
            match self.end_print().await {
                Ok(true) => {
                    if task.heartbeat_after_end() {
                        self.send_raw_packet(protocol::pkt(Cmd::Heartbeat, vec![0x01]))
                            .await?;
                    }
                    info!("print finished");
                    return Ok(());
                }
                Ok(false) | Err(Error::Timeout { .. }) => {}
                Err(error) => return Err(error),
            }
            self.maybe_sleep(self.pacing.between_end_tries()).await;
        }
        warn!("end_print did not confirm success after {attempts} tries");
        Err(Error::PrintNotConfirmed)
    }

    async fn wait_for_completion(&mut self, completion: Completion) -> Result<()> {
        let mut last_status = None;
        for _ in 0..50 {
            match completion {
                Completion::PageIndex => {
                    for packet in self.recv_pkts(self.pacing.poll_wait()).await? {
                        if packet.cmd == 0xdb {
                            return Err(Error::Printer(PrinterFault::from_u8(
                                packet.data.first().copied().unwrap_or(0),
                            )));
                        }
                        if packet.cmd == 0xe0
                            && packet
                                .data
                                .get(..2)
                                .is_some_and(|data| u16::from_be_bytes([data[0], data[1]]) >= 1)
                        {
                            return Ok(());
                        }
                    }
                }
                Completion::Status => match self
                    .transceive_with(
                        protocol::print_status(),
                        0xb3,
                        1,
                        self.pacing.poll_wait(),
                        OnTimeout::Resend,
                    )
                    .await
                {
                    Err(error @ Error::Printer(_)) => return Err(error),
                    Ok(packet) => {
                        if let Some(status) = PrintStatus::parse(&packet.data) {
                            debug!(%status, "print status");
                            if let Some(code) = status.error {
                                return Err(Error::Printer(PrinterFault::from_u8(code)));
                            }
                            last_status = Some(status);
                            if status.page_complete() {
                                return Ok(());
                            }
                        }
                    }
                    Err(_) => {}
                },
            }
            self.maybe_sleep(self.pacing.between_polls()).await;
        }
        if let Some(status) = last_status {
            warn!(%status, "printer stopped short of a complete page — check the battery");
        }
        Err(Error::PrintNotConfirmed)
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
        let max_width = self.max_width_px();
        let image = compose_for_label(path, &opts, max_width, self.profile().pixels_per_mm())?;
        let raster = image_encode::encode(image, max_width, opts.threshold.get(), opts.dither)?;

        self.preflight_ready().await?;
        self.print_raster(raster, opts.density).await
    }

    /// Print an in-memory grayscale image (dark pixels print).
    pub async fn print_gray_image(
        &mut self,
        gray: &image::GrayImage,
        density: Density,
    ) -> Result<()> {
        let raster =
            image_encode::encode_gray(gray, self.max_width_px(), Threshold::DEFAULT.get(), false)?;
        self.print_raster(raster, density).await
    }
}
