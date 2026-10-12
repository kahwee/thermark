# Build and verify thermark

Requires Rust 1.99+; rustup follows `rust-toolchain.toml`. See
[installation prerequisites](installation.md) for native transport libraries.
No printer is required for offline tests.

Maintainers: follow the [release checklist](releasing.md) before tagging or
updating the Homebrew tap. GitHub binaries, Homebrew, and crates.io are separate
distribution steps.

## Build a checkout

```sh
git clone https://github.com/kahwee/thermark.git
cd thermark
cargo build --locked
# Primary BLE path without USB serial:
scripts/build.sh --no-default-features --features ble
```

## Checks

```sh
scripts/check.sh           # format, Clippy, default-feature tests and docs
scripts/check.sh all       # also each transport feature set
scripts/check.sh render    # golden, fixture, and label placement checks
```

Use `all` for dependencies, transports, feature gates, and shared APIs. The
script rejects inherited `UPDATE_GOLDEN`; inspect `target/golden-actual/` before
accepting any baseline. See [CONTRIBUTING.md](../CONTRIBUTING.md) for useful
behavior tests and safe CLI test environments. Tests do not establish hardware support.

Build and check commands reuse existing artifacts. To reclaim space explicitly,
run `scripts/prune-build-cache.sh` from any directory. It removes this checkout's
`target/` artifacts; the next build recompiles them. Custom `CARGO_TARGET_DIR`
locations and CI caches have their own maintenance.

## Benchmarks and packet properties

```sh
cargo bench --locked --bench image_pipeline
cargo bench --locked --no-default-features --bench packet_decoder
PROPTEST_CASES=4096 cargo test --locked --no-default-features --test packet_stream
```

Compare CPU medians on the same host; measure peak RSS separately. See the
[packet measurements](packet-decoder-benchmark.md) for the bulk/small-read
tradeoff. Packet properties cover fragmented input, full payloads, corruption
recovery, and bounded retention. Commit regression seeds when fixing a failure.

## Dependency security

```sh
cargo install cargo-audit --version 0.22.2 --locked
cargo audit --deny unsound --deny yanked
```

CI audits dependency changes and runs weekly. Vulnerabilities, unsoundness and
yanked crates fail; unmaintained warnings stay visible. `ab_glyph` depends on
`ttf-parser` ([RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192));
there is no patched version. A font-engine replacement needs rendering checks.

## Code ownership and API changes

`profile.rs` owns identity/geometry/support evidence; `transport/` owns BLE/USB;
`printer/` owns job validation; `label/` and `image_encode/` own rendering.
[AGENTS.md](../AGENTS.md) gives exact boundaries and verification rules.

See [architecture](architecture.md) for module contracts, [hardware notes](hardware-notes.md)
for physical evidence, and the [changelog](../CHANGELOG.md) for API migrations.

## Prepared jobs, fonts and fault replay

See [Architecture and prepared jobs](architecture.md) for module boundaries,
artifact/journal schemas and API migration notes. The image benchmark includes
fresh versus shared font/layout rendering and Arabic shaping. Measure batch
RSS with separate processes on the same host; disk-backed preparation retains
one rendered page and a bounded glyph cache, not every page in the batch.

```sh
cargo run --locked --no-default-features --example prepared_job
cargo test --locked --test protocol_replay
python3 scripts/test_hardware_trial.py -v
```

Replay fixtures are synthetic and contain demo data. Physical success rates,
QR scanability and comparative throughput require measured hardware trials;
see [the trial guide](hardware-trial.md).

### Local measurements for 0.35.0

On the Linux x86_64 workspace with Rust 1.99, separate release-binary processes
prepared 1 and 750 identical 50x30 text labels in 0.006 and 0.346 seconds.
`resource.getrusage(RUSAGE_CHILDREN).ru_maxrss` reported 9,856 KiB for both;
this process measurement includes the launcher before exec. The 750 pages
represent roughly 69 MB of grayscale pixels when retained together. These
measurements describe offline preparation on this host, not printer throughput.

The image benchmark measured median text rendering at 0.075 ms with fresh
font/layout loading and 0.048 ms with a shared font/cache; Arabic text with the
shared font measured 0.028 ms. Re-run these medians on your host;
results depend on font, text, CPU and concurrent load. No competitor or physical
printer throughput benchmark was performed.
