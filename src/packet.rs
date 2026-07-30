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

/// Largest payload the single-byte `LEN` field can describe.
pub const MAX_DATA_LEN: usize = u8::MAX as usize;

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
    #[error("data is {len} bytes; the LEN field allows at most {MAX_DATA_LEN}")]
    DataTooLong { len: usize },
}

impl Packet {
    /// Build a packet without checking the payload length.
    ///
    /// Kept infallible for the fixed-size [`crate::protocol`] helpers, whose
    /// payloads are statically well under [`MAX_DATA_LEN`]. Any over-long
    /// payload is rejected later by [`Packet::encode`] rather than silently
    /// truncated — use [`Packet::try_new`] to reject it at the input boundary.
    pub fn new(cmd: u8, data: impl Into<Vec<u8>>) -> Self {
        Self {
            cmd,
            data: data.into(),
        }
    }

    /// Build a packet, rejecting payloads the `LEN` field cannot describe.
    ///
    /// Prefer this for data of unbounded or caller-supplied length.
    pub fn try_new(cmd: u8, data: impl Into<Vec<u8>>) -> Result<Self, PacketError> {
        let data = data.into();
        if data.len() > MAX_DATA_LEN {
            return Err(PacketError::DataTooLong { len: data.len() });
        }
        Ok(Self { cmd, data })
    }

    /// XOR of `CMD`, `LEN`, and every data byte.
    ///
    /// Takes `len` explicitly so the checksum can never be computed over a
    /// length that disagrees with the one written to the wire.
    pub fn checksum(cmd: u8, len: u8, data: &[u8]) -> u8 {
        let mut c = cmd ^ len;
        for b in data {
            c ^= *b;
        }
        c
    }

    /// Encode to on-wire bytes.
    ///
    /// Fails with [`PacketError::DataTooLong`] rather than truncating the
    /// length field and emitting an undecodable frame.
    pub fn encode(&self) -> Result<Vec<u8>, PacketError> {
        let len = u8::try_from(self.data.len()).map_err(|_| PacketError::DataTooLong {
            len: self.data.len(),
        })?;
        let sum = Self::checksum(self.cmd, len, &self.data);
        let mut out = Vec::with_capacity(7 + self.data.len());
        out.extend_from_slice(&HEAD);
        out.push(self.cmd);
        out.push(len);
        out.extend_from_slice(&self.data);
        out.push(sum);
        out.extend_from_slice(&TAIL);
        Ok(out)
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
        // `len` came from a single byte, so the cast back is lossless.
        let expected = Self::checksum(cmd, len as u8, &data);
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
        assert_eq!(p.encode().unwrap(), hex::decode("55551a01011aaaaa").unwrap());
    }

    #[test]
    fn roundtrip() {
        let p = Packet::new(0x40, vec![0x0b]);
        let enc = p.encode().unwrap();
        let d = Packet::decode(&enc).unwrap();
        assert_eq!(d, p);
    }

    #[test]
    fn drain_fragmented() {
        let a = Packet::new(0x1a, vec![0x01]).encode().unwrap();
        let b = Packet::new(0x40, vec![0x0b]).encode().unwrap();
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
        let enc = p.encode().unwrap();
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
        let enc = p.encode().unwrap();
        assert_eq!(enc[3], 6);
        assert_eq!(Packet::decode(&enc).unwrap().data.len(), 6);
    }

    #[test]
    fn oversized_payload_is_rejected_not_truncated() {
        // 300 bytes used to encode with LEN=44 and a checksum over the wrong
        // length, producing a frame no receiver could decode.
        let p = Packet::new(0x85, vec![0xAB; 300]);
        assert!(matches!(
            p.encode(),
            Err(PacketError::DataTooLong { len: 300 })
        ));
        assert!(matches!(
            Packet::try_new(0x85, vec![0xAB; 300]),
            Err(PacketError::DataTooLong { len: 300 })
        ));
    }

    #[test]
    fn max_length_payload_still_roundtrips() {
        let p = Packet::try_new(0x85, vec![0xAB; MAX_DATA_LEN]).expect("255 bytes fits");
        let enc = p.encode().unwrap();
        assert_eq!(enc[3] as usize, MAX_DATA_LEN);
        assert_eq!(Packet::decode(&enc).unwrap(), p);
    }

    #[test]
    fn drain_garbage_never_panics_and_clears() {
        let mut buf = vec![0u8, 1, 2, 0xAA, 0x55, 0xFF];
        let pkts = Packet::drain_buffer(&mut buf);
        assert!(pkts.is_empty());
        // No complete head → buffer cleared
        assert!(buf.is_empty());
    }

    #[test]
    fn drain_recovers_valid_after_noise() {
        let good = Packet::new(0x1a, vec![0x01]).encode().unwrap();
        let mut buf = vec![0xDE, 0xAD, 0xBE, 0xEF];
        buf.extend_from_slice(&good);
        let pkts = Packet::drain_buffer(&mut buf);
        assert_eq!(pkts.len(), 1);
        assert_eq!(pkts[0].cmd, 0x1a);
        assert!(buf.is_empty());
    }

    #[test]
    fn drain_bad_checksum_skips_and_continues() {
        let good = Packet::new(0x40, vec![0x0b]).encode().unwrap();
        let mut bad = good.clone();
        // Flip checksum byte (second-to-last before tail)
        let cs = bad.len() - 3;
        bad[cs] ^= 0xFF;
        let mut buf = bad;
        buf.extend_from_slice(&good);
        let pkts = Packet::drain_buffer(&mut buf);
        assert_eq!(pkts.len(), 1);
        assert_eq!(pkts[0].cmd, 0x40);
    }

    /// Property: random noise never panics; output packets always re-encode cleanly.
    #[test]
    fn prop_drain_random_noise_no_panic() {
        use proptest::prelude::*;
        proptest::proptest!(|(noise in prop::collection::vec(any::<u8>(), 0..512))| {
            let mut buf = noise;
            let pkts = Packet::drain_buffer(&mut buf);
            for p in pkts {
                let enc = p.encode().unwrap();
                let dec = Packet::decode(&enc).expect("roundtrip after drain");
                prop_assert_eq!(dec, p);
            }
            // leftover is either empty or a proper incomplete frame starting at HEAD
            if !buf.is_empty() {
                prop_assert!(buf.len() < 7 || buf.starts_with(&HEAD));
            }
        });
    }

    /// Property: a valid packet is recovered when preceded by noise that cannot
    /// form a frame head (`55 55`).
    #[test]
    fn prop_valid_packet_survives_noise_wrap() {
        use proptest::prelude::*;
        // Bytes that never form HEAD when consecutive (exclude 0x55 entirely).
        let noise = prop::collection::vec(0u8..=0x54u8, 0..24);
        proptest::proptest!(|(
            cmd in any::<u8>(),
            data in prop::collection::vec(any::<u8>(), 0..32),
            prefix in noise.clone(),
            suffix in noise,
        )| {
            let p = Packet::new(cmd, data);
            let enc = p.encode().unwrap();
            let mut buf = prefix;
            buf.extend_from_slice(&enc);
            buf.extend_from_slice(&suffix);
            let pkts = Packet::drain_buffer(&mut buf);
            prop_assert!(
                pkts.iter().any(|x| x == &p),
                "lost packet among cmds {:?}",
                pkts.iter().map(|x| x.cmd).collect::<Vec<_>>()
            );
        });
    }
}
