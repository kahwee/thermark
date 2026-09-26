//! Packet-stream invariants independent of transport and hardware.
use proptest::prelude::*;
use thermark::Packet;
use thermark::packet::{MAX_DATA_LEN, MAX_FRAME_LEN, PacketDecoder};

proptest! {
    #[test]
    fn valid_stream_survives_arbitrary_fragmentation(
        entries in prop::collection::vec(
            (any::<u8>(), prop::collection::vec(any::<u8>(), 0..=MAX_DATA_LEN)),
            0..40,
        ),
        sizes in prop::collection::vec(1usize..=MAX_FRAME_LEN * 2, 1..40),
    ) {
        let expected: Vec<_> = entries.into_iter().map(|(cmd, data)| Packet::new(cmd, data)).collect();
        let bytes: Vec<_> = expected.iter().flat_map(|p| p.encode().unwrap()).collect();
        let mut decoder = PacketDecoder::new();
        let mut actual = Vec::new();
        let mut remaining = bytes.as_slice();
        for size in sizes.iter().cycle() {
            if remaining.is_empty() {
                break;
            }
            let (chunk, rest) = remaining.split_at((*size).min(remaining.len()));
            actual.extend(decoder.push(chunk));
            prop_assert!(decoder.buffered_len() < MAX_FRAME_LEN);
            remaining = rest;
        }
        prop_assert_eq!(&actual, &expected);
        prop_assert_eq!(decoder.buffered_len(), 0);
    }

    #[test]
    fn arbitrary_bytes_decode_identically_across_read_boundaries(
        bytes in prop::collection::vec(any::<u8>(), 0..8_192),
        chunk_size in 1usize..=MAX_FRAME_LEN * 2,
    ) {
        let mut bulk = bytes.clone();
        let expected = Packet::drain_buffer(&mut bulk);
        let mut fragmented = PacketDecoder::new();
        let mut actual = Vec::new();
        for chunk in bytes.chunks(chunk_size) {
            actual.extend(fragmented.push(chunk));
            prop_assert!(fragmented.buffered_len() < MAX_FRAME_LEN);
        }
        prop_assert_eq!(&actual, &expected);
        prop_assert_eq!(fragmented.buffered_len(), bulk.len());
        for packet in actual {
            prop_assert_eq!(Packet::decode(&packet.encode().unwrap()).unwrap(), packet);
        }
    }

    #[test]
    fn corrupt_frame_cannot_hide_a_following_valid_frame(
        cmd in any::<u8>(),
        data in prop::collection::vec(any::<u8>(), 0..=MAX_DATA_LEN),
        chunk_size in 1usize..=MAX_FRAME_LEN,
    ) {
        let expected = Packet::new(cmd, data);
        // Fixed payload without embedded headers makes corruption unambiguous.
        let mut bytes = Packet::new(0x40, [1, 2, 3]).encode().unwrap();
        bytes[7] ^= 1; // checksum
        bytes.extend(expected.encode().unwrap());
        let mut decoder = PacketDecoder::new();
        let actual: Vec<_> = bytes.chunks(chunk_size).flat_map(|c| decoder.push(c)).collect();
        prop_assert_eq!(actual, vec![expected]);
        prop_assert_eq!(decoder.buffered_len(), 0);
    }
}
