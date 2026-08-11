//! Read-only printer information queries and response aggregation.

use super::client::PrinterClient;
use super::info::{Heartbeat, InfoValue, PrinterSummary, RfidInfo};
use super::raw::OnTimeout;
use crate::errors::{Error, Result};
use crate::protocol::{self, Cmd, InfoKey};
use crate::transport::Transport;
use std::time::Duration;

impl<T: Transport> PrinterClient<T> {
    pub async fn get_info(&mut self, key: InfoKey) -> Result<InfoValue> {
        let request = protocol::info(key);
        let response_cmd = (Cmd::PrinterInfo as u8).wrapping_add(key as u8);
        let packet = self
            .transceive_with(
                request,
                response_cmd,
                8,
                Duration::from_millis(200),
                OnTimeout::Resend,
            )
            .await?;
        Ok(InfoValue::parse(key, &packet.data))
    }

    pub async fn heartbeat(&mut self) -> Result<Heartbeat> {
        if let Ok(packet) = self
            .transceive_offset(Cmd::Heartbeat as u8, vec![0x01], 1, OnTimeout::Resend)
            .await
        {
            return Ok(Heartbeat::parse(&packet.data));
        }

        self.send_pkt(&protocol::heartbeat()).await?;
        let packets = self.recv_pkts(Duration::from_millis(500)).await?;
        let packet = packets
            .into_iter()
            .find(|packet| matches!(packet.cmd, 0xdd | 0xde | 0xdf | 0xd9))
            .ok_or_else(|| Error::msg("no heartbeat response"))?;
        Ok(Heartbeat::parse(&packet.data))
    }

    pub async fn rfid_info(&mut self) -> Result<RfidInfo> {
        let packet = self
            .transceive_with(
                protocol::rfid(),
                0x1b,
                8,
                Duration::from_millis(250),
                OnTimeout::Resend,
            )
            .await?;
        Ok(RfidInfo::parse(&packet.data))
    }

    pub async fn fetch_summary(&mut self) -> Result<PrinterSummary> {
        fn optional<T>(result: Result<T>, errors: &mut Vec<Error>) -> Option<T> {
            match result {
                Ok(value) => Some(value),
                Err(error) => {
                    errors.push(error);
                    None
                }
            }
        }

        let mut errors = Vec::new();
        let serial = optional(self.get_info(InfoKey::DeviceSerial).await, &mut errors);
        let soft = optional(self.get_info(InfoKey::SoftVersion).await, &mut errors);
        let hard = optional(self.get_info(InfoKey::HardVersion).await, &mut errors);
        let battery = optional(self.get_info(InfoKey::Battery).await, &mut errors);
        let device_type = optional(self.get_info(InfoKey::DeviceType).await, &mut errors);
        let heartbeat = optional(self.heartbeat().await, &mut errors);
        let rfid = optional(self.rfid_info().await, &mut errors);
        let summary = PrinterSummary {
            serial,
            soft,
            hard,
            battery,
            device_type,
            heartbeat,
            rfid,
        };
        if summary.has_data() {
            Ok(summary)
        } else {
            Err(errors
                .into_iter()
                .next()
                .unwrap_or_else(|| Error::msg("printer returned no summary data")))
        }
    }
}
