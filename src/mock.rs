//! In-memory transport for tests (no Bluetooth / USB).

use crate::errors::{Error, Result};
use crate::packet::Packet;
use crate::transport::Transport;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Records every TX packet and synthesizes RX replies.
///
/// By default responds with success to common commands. Use
/// [`MockTransport::fail_cmd`] to inject `0xDB` printer errors,
/// [`MockTransport::reject_cmd`] for ACK-with-failure (`0x00` payload),
/// [`MockTransport::mute_cmd`] to skip auto-replies (e.g. PrintEnd),
/// or [`MockTransport::set_heartbeat`] to control preflight sensors.
pub struct MockTransport {
    /// Encoded packets sent by the client (full frames).
    pub tx: Vec<Vec<u8>>,
    /// Parsed TX packets (cmd + data).
    pub tx_packets: Vec<Packet>,
    rx_queue: Vec<Vec<u8>>,
    /// cmd → error code for In_PrintError (0xDB)
    fail_on: HashMap<u8, u8>,
    /// Commands that auto-reply with success payload `0x00` (rejected).
    reject: HashSet<u8>,
    /// Commands that get no auto-reply (timeouts).
    mute: HashSet<u8>,
    /// cmd → remaining sends to swallow entirely (simulated lost writes).
    drop_writes: HashMap<u8, u32>,
    /// If true, auto-generate success replies (default).
    auto_reply: bool,
    /// Override 13-byte heartbeat payload (closing, power, paper, rfid at 9..=12).
    heartbeat: Option<[u8; 13]>,
    /// Override the `PrintStatus` (0xa3) reply body.
    print_status: Option<Vec<u8>>,
    recv_error: Option<String>,
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
            reject: HashSet::new(),
            mute: HashSet::new(),
            drop_writes: HashMap::new(),
            auto_reply: true,
            heartbeat: None,
            print_status: None,
            recv_error: None,
        }
    }

    /// Set the `PrintStatus` (0xa3) reply body.
    ///
    /// The 10-byte form is `[page:u16, print%, feed%, _, _, error, _, _, _]`.
    /// The default reports a complete page with no fault; override it to model
    /// a printer that stalls part-way, which is what a sagging battery looks
    /// like on the wire.
    pub fn set_print_status(&mut self, body: Vec<u8>) -> &mut Self {
        self.print_status = Some(body);
        self
    }

    /// Next time `cmd` is sent, reply with `0xDB` / `error_code` instead of success.
    pub fn fail_cmd(&mut self, cmd: u8, error_code: u8) -> &mut Self {
        self.fail_on.insert(cmd, error_code);
        self
    }

    /// Auto-reply to `cmd` with a normal response command but payload `0x00` (NACK).
    pub fn reject_cmd(&mut self, cmd: u8) -> &mut Self {
        self.reject.insert(cmd);
        self
    }

    /// Do not auto-reply to `cmd` (simulates missing ACK / timeout).
    pub fn mute_cmd(&mut self, cmd: u8) -> &mut Self {
        self.mute.insert(cmd);
        self
    }

    /// Swallow the first `n` sends of `cmd`, then behave normally.
    ///
    /// Simulates a lost BLE write: the printer never saw the request, so only a
    /// resend can recover it. The frame is still recorded in [`Self::tx`], so
    /// tests can assert how many attempts went out.
    pub fn drop_first_writes(&mut self, cmd: u8, n: u32) -> &mut Self {
        self.drop_writes.insert(cmd, n);
        self
    }

    /// Set 13-byte heartbeat body: indices 9=cover, 10=power, 11=paper, 12=rfid.
    ///
    /// Defaults when unset: cover closed (0), power 3, paper ok (0), rfid ok (1).
    pub fn set_heartbeat(&mut self, payload: [u8; 13]) -> &mut Self {
        self.heartbeat = Some(payload);
        self
    }

    /// Convenience: cover open, paper missing, or battery empty for preflight tests.
    pub fn heartbeat_not_ready_cover_open(&mut self) -> &mut Self {
        let mut d = [0u8; 13];
        d[9] = 1; // cover open
        d[10] = 3;
        d[11] = 0;
        d[12] = 1;
        self.set_heartbeat(d)
    }

    pub fn heartbeat_not_ready_no_paper(&mut self) -> &mut Self {
        let mut d = [0u8; 13];
        d[9] = 0;
        d[10] = 3;
        d[11] = 1; // no paper
        d[12] = 1;
        self.set_heartbeat(d)
    }

    pub fn push_rx(&mut self, packet: Packet) {
        self.rx_queue
            .push(packet.encode().expect("mock RX packet must encode"));
    }

    /// Queue a raw fragment to exercise transport-level packet splitting.
    pub fn push_rx_raw(&mut self, bytes: impl Into<Vec<u8>>) {
        self.rx_queue.push(bytes.into());
    }

    pub fn fail_receives(&mut self, message: impl Into<String>) -> &mut Self {
        self.recv_error = Some(message.into());
        self
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

    fn synthesize_reply(&self, cmd: u8, data: &[u8]) -> Option<Packet> {
        match cmd {
            0x83..=0x85 => None,
            0x21 => Some(Packet::new(0x31, vec![0x01])),
            0x23 => Some(Packet::new(0x33, vec![0x01])),
            0x01 => Some(Packet::new(0x02, vec![0x01])),
            0x03 => Some(Packet::new(0x04, vec![0x01])),
            0x13 => Some(Packet::new(0x14, vec![0x01, 0x00])),
            0x15 => Some(Packet::new(0x16, vec![0x01])),
            0x20 => Some(Packet::new(0x30, vec![0x01])),
            0xe3 => Some(Packet::new(0xe4, vec![0x01])),
            0xf3 => Some(Packet::new(0xf4, vec![0x01])),
            0xa3 => Some(Packet::new(
                0xb3,
                self.print_status.clone().unwrap_or_else(|| {
                    vec![0x00, 0x01, 0x64, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
                }),
            )),
            0xdc => {
                let mut d = vec![0u8; 13];
                if let Some(hb) = self.heartbeat {
                    d.copy_from_slice(&hb);
                } else {
                    d[9] = 0;
                    d[10] = 3;
                    d[11] = 0;
                    d[12] = 1;
                }
                Some(Packet::new(0xdd, d))
            }
            0x1a => Some(Packet::new(0x1b, vec![0x00])),
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
            other => Some(Packet::new(other.wrapping_add(1), vec![0x01])),
        }
    }
}

impl Transport for MockTransport {
    async fn send_raw(&mut self, data: &[u8]) -> Result<()> {
        self.tx.push(data.to_vec());
        let pkt = Packet::decode(data).map_err(|e| {
            Error::msg(format!(
                "mock: client sent undecodable frame: {e} ({})",
                hex::encode(data)
            ))
        })?;
        let cmd = pkt.cmd;
        let pdata = pkt.data.clone();
        self.tx_packets.push(pkt);

        // A lost write: recorded as sent, but the printer never sees it.
        if let Some(remaining) = self.drop_writes.get_mut(&cmd)
            && *remaining > 0
        {
            *remaining -= 1;
            return Ok(());
        }

        if let Some(code) = self.fail_on.get(&cmd).copied() {
            self.push_rx(Packet::new(0xdb, vec![code]));
            return Ok(());
        }

        if self.mute.contains(&cmd) {
            return Ok(());
        }

        if self.auto_reply
            && let Some(mut reply) = self.synthesize_reply(cmd, &pdata)
        {
            if self.reject.contains(&cmd) {
                reply.data = vec![0x00];
            }
            self.push_rx(reply);
        }
        Ok(())
    }

    async fn recv_raw(&mut self, _wait: Duration) -> Result<Vec<u8>> {
        if let Some(message) = &self.recv_error {
            return Err(Error::transport(message.clone()));
        }
        if self.rx_queue.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.rx_queue.remove(0))
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
        let rx = m.recv_raw(Duration::from_millis(1)).await.unwrap();
        let packet = Packet::decode(&rx).unwrap();
        assert_eq!(packet.cmd, 0x31);
        assert_eq!(packet.data, vec![0x01]);
    }

    #[tokio::test]
    async fn injects_print_error() {
        let mut m = MockTransport::new();
        m.fail_cmd(0x01, 0x02);
        m.send_packet(&crate::PrintTask::Simple.print_start(1))
            .await
            .unwrap();
        let rx = m.recv_raw(Duration::from_millis(1)).await.unwrap();
        let packet = Packet::decode(&rx).unwrap();
        assert_eq!(packet.cmd, 0xdb);
        assert_eq!(packet.data, vec![0x02]);
    }

    #[tokio::test]
    async fn mute_cmd_skips_reply() {
        let mut m = MockTransport::new();
        m.mute_cmd(0xf3);
        m.send_packet(&protocol::print_end()).await.unwrap();
        let rx = m.recv_raw(Duration::from_millis(1)).await.unwrap();
        assert!(rx.is_empty());
    }
}
