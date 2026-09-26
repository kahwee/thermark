# Packet decoder benchmark

Measured locally on macOS ARM64 with Rust 1.98.1 on 2026-09-25. Both versions
used the same release benchmark: 31 samples per case, median wall time, a
48-byte payload per frame, no transports, and no printer. The original binary
was retained and rerun after the updated binary finished, without concurrent
builds. These are local observations, not CI performance thresholds.

| Frames | Bytes per read | Before (ms) | After (ms) |
| ---: | ---: | ---: | ---: |
| 128 | 20 | 0.009 | 0.010 |
| 128 | 7,040 | 0.009 | 0.004 |
| 1,024 | 20 | 0.076 | 0.079 |
| 1,024 | 56,320 | 0.383 | 0.030 |
| 8,192 | 20 | 0.626 | 0.681 |
| 8,192 | 450,560 | 39.515 | 0.246 |

The old decoder shifted the remaining buffer after every frame, making large
batches quadratic. The new decoder advances a cursor and compacts once per
batch. `PacketDecoder::push` processes input in bounded chunks so a large read
does not leave a large receive allocation behind. Returned packets still
require memory proportional to the number of packets; the bound applies to
retained input, not output.

The largest bulk-read case improved about 160-fold. Small reads were slightly
slower (about 9% in the largest 20-byte-read case); their absolute time remains
below one millisecond for 8,192 frames. This benchmark does not measure BLE,
USB, printer throughput, or hardware correctness.

Reproduce with:

```bash
cargo bench --locked --no-default-features --bench packet_decoder
```

Compare revisions on the same host with other CPU-heavy jobs stopped. Keep
rendering benchmarks separate with `cargo bench --locked --bench image_pipeline`.
