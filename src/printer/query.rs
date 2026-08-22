//! Read-only printer information queries and response aggregation.

use super::client::PrinterClient;
use super::info::{Heartbeat, InfoValue, PrinterSummary, RfidInfo};
use super::raw::OnTimeout;
use crate::errors::{Error, Result};
use crate::profile::PrinterIdentity;
use crate::protocol::{self, Cmd, InfoKey};
use crate::transport::Transport;
use std::time::Duration;

impl<T: Transport> PrinterClient<T> {
    pub async fn identify(&mut self) -> Result<PrinterIdentity> {
        let connect_result = self
            .transceive_with(
                protocol::connect(),
                0xc2,
                1,
                Duration::from_millis(250),
                OnTimeout::Resend,
            )
            .await
            .ok()
            .and_then(|packet| packet.data.first().copied());
        let model_packet = self
            .transceive_with(
                protocol::info(InfoKey::DeviceType),
                0x48,
                8,
                Duration::from_millis(200),
                OnTimeout::Resend,
            )
            .await?;
        let model_id = match model_packet.data.as_slice() {
            [value] => u16::from(*value) << 8,
            [hi, lo] => u16::from_be_bytes([*hi, *lo]),
            data => {
                return Err(Error::msg(format!(
                    "invalid model-id response: {} bytes",
                    data.len()
                )));
            }
        };
        let protocol_version = match connect_result {
            Some(1 | 2) => connect_result,
            Some(3) => self
                .transceive_with(
                    protocol::printer_status_data(),
                    0xb5,
                    2,
                    Duration::from_millis(250),
                    OnTimeout::Resend,
                )
                .await
                .ok()
                .and_then(|packet| parse_protocol_version(&packet.data)),
            _ => None,
        };
        let firmware = self
            .get_info(InfoKey::SoftVersion)
            .await
            .ok()
            .map(|v| v.to_string());
        let hardware = self
            .get_info(InfoKey::HardVersion)
            .await
            .ok()
            .map(|v| v.to_string());
        Ok(PrinterIdentity {
            model_id,
            protocol_version,
            firmware,
            hardware,
        })
    }

    pub fn apply_identity(
        &mut self,
        identity: &PrinterIdentity,
        update_task: bool,
    ) -> Option<&'static crate::profile::PrinterProfile> {
        let profile = crate::profile::profile_for_identity(identity)?;
        self.model = profile.model;
        self.profile = profile;
        if update_task && let Some(task) = crate::profile::task_for_identity(identity) {
            self.task = task;
        }
        Some(profile)
    }
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

fn parse_protocol_version(data: &[u8]) -> Option<u8> {
    if data.len() < 13 {
        return None;
    }
    let raw = u16::from(data[11]) * 100 + u16::from(data[12]);
    Some(match raw {
        204..=299 => 3,
        300..=301 => 4,
        302.. => 5,
        _ => return None,
    })
}

#[cfg(test)]
mod identity_tests {
    use super::parse_protocol_version;
    #[test]
    fn parses_protocol_generations() {
        for (raw, expected) in [(204, 3), (300, 4), (302, 5)] {
            let mut data = [0u8; 13];
            data[11] = (raw / 100) as u8;
            data[12] = (raw % 100) as u8;
            assert_eq!(parse_protocol_version(&data), Some(expected));
        }
        assert_eq!(parse_protocol_version(&[0; 13]), None);
    }
}
