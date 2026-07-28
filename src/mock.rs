//! In-memory transport for tests (no Bluetooth / USB).

use crate::packet::Packet;
use crate::transport::Transport;
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

/// Records every TX packet and synthesizes RX replies.
///
/// By default responds with success to common commands. Use
/// [`MockTransport::fail_cmd`] to inject `0xDB` printer errors.
pub struct MockTransport {
    /// Encoded packets sent by the client (full frames).
    pub tx: Vec<Vec<u8>>,
    /// Parsed TX packets (cmd + data).
    pub tx_packets: Vec<Packet>,
    rx_queue: Vec<Packet>,
    /// cmd → error code for In_PrintError (0xDB)
    fail_on: HashMap<u8, u8>,
    /// If true, auto-generate success replies (default).
    auto_reply: bool,
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            tx: Vec::new(),
            tx_packets: Vec::new(),
            rx_queue: Vec::new(),
            fail_on: HashMap::new(),
            auto_reply: true,
        }
    }

    /// Next time `cmd` is sent, reply with `0xDB` / `error_code` instead of success.
    pub fn fail_cmd(&mut self, cmd: u8, error_code: u8) -> &mut Self {
        self.fail_on.insert(cmd, error_code);
        self
    }

    pub fn push_rx(&mut self, packet: Packet) {
        self.rx_queue.push(packet);
    }

    pub fn auto_reply(&mut self, yes: bool) -> &mut Self {
        self.auto_reply = yes;
        self
    }

    /// Commands seen on the wire (order preserved).
    pub fn tx_cmds(&self) -> Vec<u8> {
        self.tx_packets.iter().map(|p| p.cmd).collect()
    }

    /// First TX packet with this command, if any.
    pub fn first_tx(&self, cmd: u8) -> Option<&Packet> {
        self.tx_packets.iter().find(|p| p.cmd == cmd)
    }

    fn synthesize_reply(cmd: u8, data: &[u8]) -> Option<Packet> {
        match cmd {
            // one-way image data
            0x83..=0x85 => None,
            0x21 => Some(Packet::new(0x31, vec![0x01])), // SetDensity
            0x23 => Some(Packet::new(0x33, vec![0x01])), // SetLabelType
            0x01 => Some(Packet::new(0x02, vec![0x01])), // PrintStart
            0x03 => Some(Packet::new(0x04, vec![0x01])), // PageStart
            0x13 => Some(Packet::new(0x14, vec![0x01, 0x00])), // SetPageSize
            0x15 => Some(Packet::new(0x16, vec![0x01])),
            0x20 => Some(Packet::new(0x30, vec![0x01])), // PrintClear
            0xe3 => Some(Packet::new(0xe4, vec![0x01])), // PageEnd
            0xf3 => Some(Packet::new(0xf4, vec![0x01])), // PrintEnd
            0xa3 => {
                // PrintStatus — page done-ish
                // layout loosely: page u16, progress bytes…
                Some(Packet::new(
                    0xb3,
                    vec![0x00, 0x01, 0x64, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
                ))
            }
            0xdc => {
                // Heartbeat Advanced1 13-byte style: lid=0 paper=0 rfid=1 power=3
                // indices used by Heartbeat::parse for len 13
                let mut d = vec![0u8; 13];
                d[9] = 0; // lid closed
                d[10] = 3; // power
                d[11] = 0; // paper inserted
                d[12] = 1; // rfid ok
                Some(Packet::new(0xdd, d))
            }
            0x1a => Some(Packet::new(0x1b, vec![0x00])), // no RFID tag
            0x40 => {
                let key = data.first().copied().unwrap_or(0);
                let body = match key {
                    0x0b => b"TESTMOCK01".to_vec(),
                    0x09 | 0x0c => vec![0x05, 0x00],
                    0x0a => vec![0x03],
                    0x08 => vec![0x10, 0x00],
                    _ => vec![0x01],
                };
                Some(Packet::new(0x40u8.wrapping_add(key), body))
            }
            // generic ack
            other => Some(Packet::new(other.wrapping_add(1), vec![0x01])),
        }
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn send_raw(&mut self, data: &[u8]) -> Result<()> {
        self.tx.push(data.to_vec());
        let pkt = match Packet::decode(data) {
            Ok(p) => p,
            Err(e) => bail!("mock: client sent undecodable frame: {e} ({})", hex::encode(data)),
        };
        let cmd = pkt.cmd;
        let pdata = pkt.data.clone();
        self.tx_packets.push(pkt);

        if let Some(code) = self.fail_on.get(&cmd).copied() {
            self.rx_queue.push(Packet::new(0xdb, vec![code]));
            return Ok(());
        }

        if self.auto_reply {
            if let Some(reply) = Self::synthesize_reply(cmd, &pdata) {
                self.rx_queue.push(reply);
            }
        }
        Ok(())
    }

    async fn recv_packets(&mut self, _wait: Duration) -> Result<Vec<Packet>> {
        if self.rx_queue.is_empty() {
            return Ok(vec![]);
        }
        Ok(std::mem::take(&mut self.rx_queue))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol;

    #[tokio::test]
    async fn records_tx_and_auto_acks_density() {
        let mut m = MockTransport::new();
        m.send_packet(&protocol::set_density(4)).await.unwrap();
        assert_eq!(m.tx_cmds(), vec![0x21]);
        let rx = m.recv_packets(Duration::from_millis(1)).await.unwrap();
        assert_eq!(rx.len(), 1);
        assert_eq!(rx[0].cmd, 0x31);
        assert_eq!(rx[0].data, vec![0x01]);
    }

    #[tokio::test]
    async fn injects_print_error() {
        let mut m = MockTransport::new();
        m.fail_cmd(0x01, 0x02);
        m.send_packet(&protocol::print_start_simple()).await.unwrap();
        let rx = m.recv_packets(Duration::from_millis(1)).await.unwrap();
        assert_eq!(rx[0].cmd, 0xdb);
        assert_eq!(rx[0].data, vec![0x02]);
    }
}
