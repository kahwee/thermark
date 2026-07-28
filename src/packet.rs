//! NIIMBOT binary packet framing.
//!
//! ```text
//! 0x55 0x55 | CMD | LEN | DATA… | CHECKSUM | 0xAA 0xAA
//! ```
//!
//! Checksum = XOR of CMD, LEN, and every DATA byte.

use thiserror::Error;

pub const HEAD: [u8; 2] = [0x55, 0x55];
pub const TAIL: [u8; 2] = [0xAA, 0xAA];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub cmd: u8,
    pub data: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum PacketError {
    #[error("packet too short ({0} bytes)")]
    TooShort(usize),
    #[error("invalid head (expected 55 55)")]
    BadHead,
    #[error("invalid tail (expected AA AA)")]
    BadTail,
    #[error("length field {claimed} does not match buffer")]
    BadLength { claimed: usize },
    #[error("checksum mismatch: got {got:#04x}, expected {expected:#04x}")]
    BadChecksum { got: u8, expected: u8 },
}

impl Packet {
    pub fn new(cmd: u8, data: impl Into<Vec<u8>>) -> Self {
        Self {
            cmd,
            data: data.into(),
        }
    }

    pub fn checksum(cmd: u8, data: &[u8]) -> u8 {
        let mut c = cmd ^ (data.len() as u8);
        for b in data {
            c ^= *b;
        }
        c
    }

    /// Encode to on-wire bytes.
    pub fn encode(&self) -> Vec<u8> {
        let len = self.data.len() as u8;
        let sum = Self::checksum(self.cmd, &self.data);
        let mut out = Vec::with_capacity(7 + self.data.len());
        out.extend_from_slice(&HEAD);
        out.push(self.cmd);
        out.push(len);
        out.extend_from_slice(&self.data);
        out.push(sum);
        out.extend_from_slice(&TAIL);
        out
    }

    /// Decode a single complete packet from `buf`.
    pub fn decode(buf: &[u8]) -> Result<Self, PacketError> {
        if buf.len() < 7 {
            return Err(PacketError::TooShort(buf.len()));
        }
        if buf[0] != HEAD[0] || buf[1] != HEAD[1] {
            return Err(PacketError::BadHead);
        }
        let cmd = buf[2];
        let len = buf[3] as usize;
        let total = 7 + len;
        if buf.len() < total {
            return Err(PacketError::BadLength { claimed: len });
        }
        if buf[total - 2] != TAIL[0] || buf[total - 1] != TAIL[1] {
            return Err(PacketError::BadTail);
        }
        let data = buf[4..4 + len].to_vec();
        let got = buf[4 + len];
        let expected = Self::checksum(cmd, &data);
        if got != expected {
            return Err(PacketError::BadChecksum { got, expected });
        }
        Ok(Self { cmd, data })
    }

    /// Pull zero or more complete packets from a growing receive buffer.
    pub fn drain_buffer(buf: &mut Vec<u8>) -> Vec<Self> {
        let mut packets = Vec::new();
        loop {
            // Resync to head if needed
            if let Some(pos) = buf.windows(2).position(|w| w == HEAD) {
                if pos > 0 {
                    buf.drain(..pos);
                }
            } else {
                buf.clear();
                break;
            }

            if buf.len() < 7 {
                break;
            }
            let len = buf[3] as usize;
            let total = 7 + len;
            if buf.len() < total {
                break;
            }
            match Self::decode(&buf[..total]) {
                Ok(p) => {
                    packets.push(p);
                    buf.drain(..total);
                }
                Err(_) => {
                    // Skip one byte and resync
                    buf.drain(..1);
                }
            }
        }
        packets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_simple_rfid() {
        // 55 55 1a 01 01 1a aa aa
        let p = Packet::new(0x1a, vec![0x01]);
        assert_eq!(p.encode(), hex::decode("55551a01011aaaaa").unwrap());
    }

    #[test]
    fn roundtrip() {
        let p = Packet::new(0x40, vec![0x0b]);
        let enc = p.encode();
        let d = Packet::decode(&enc).unwrap();
        assert_eq!(d, p);
    }

    #[test]
    fn drain_fragmented() {
        let a = Packet::new(0x1a, vec![0x01]).encode();
        let b = Packet::new(0x40, vec![0x0b]).encode();
        let mut buf = Vec::new();
        buf.extend_from_slice(&a[..4]);
        assert!(Packet::drain_buffer(&mut buf).is_empty());
        buf.extend_from_slice(&a[4..]);
        buf.extend_from_slice(&b);
        let pkts = Packet::drain_buffer(&mut buf);
        assert_eq!(pkts.len(), 2);
        assert_eq!(pkts[0].cmd, 0x1a);
        assert_eq!(pkts[1].cmd, 0x40);
        assert!(buf.is_empty());
    }

    #[test]
    fn print_start_b1_seven_bytes() {
        // B1 PrintStart payload from community wiki
        let data = [0x00u8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
        let p = Packet::new(0x01, data);
        let enc = p.encode();
        assert_eq!(&enc[..2], &[0x55, 0x55]);
        assert_eq!(enc[2], 0x01);
        assert_eq!(enc[3], 7);
        assert_eq!(&enc[enc.len() - 2..], &[0xAA, 0xAA]);
        assert_eq!(Packet::decode(&enc).unwrap(), p);
    }

    #[test]
    fn set_page_size_6b_checksum() {
        // rows=240 (0x00f0), cols=384 (0x0180), copies=1
        let mut data = Vec::new();
        data.extend_from_slice(&240u16.to_be_bytes());
        data.extend_from_slice(&384u16.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        let p = Packet::new(0x13, data);
        let enc = p.encode();
        assert_eq!(enc[3], 6);
        assert_eq!(Packet::decode(&enc).unwrap().data.len(), 6);
    }
}
