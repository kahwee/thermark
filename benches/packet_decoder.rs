//! CPU-only decoder scaling; run with `cargo bench --locked --bench packet_decoder`.
use std::hint::black_box;
use std::time::Instant;
use thermark::Packet;
use thermark::packet::PacketDecoder;

fn main() {
    let frame = Packet::new(0x85, vec![0x42; 48]).encode().unwrap();
    for count in [128, 1_024, 8_192] {
        let stream = frame.repeat(count);
        for chunk_size in [20, stream.len()] {
            let mut samples = Vec::with_capacity(31);
            for _ in 0..31 {
                let started = Instant::now();
                let mut decoder = PacketDecoder::new();
                let mut decoded = 0;
                for chunk in black_box(&stream).chunks(chunk_size) {
                    decoded += black_box(decoder.push(chunk)).len();
                }
                samples.push(started.elapsed());
                assert_eq!(decoded, count);
                assert_eq!(decoder.buffered_len(), 0);
            }
            samples.sort_unstable();
            println!(
                "{count:>5} frames / {chunk_size:>6} byte reads: {:>10.3} ms median",
                samples[samples.len() / 2].as_secs_f64() * 1_000.0
            );
        }
    }
}
