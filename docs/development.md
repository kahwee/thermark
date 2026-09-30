# Build and verify thermark

Requires Rust 1.98+; rustup follows `rust-toolchain.toml`. See
[installation prerequisites](installation.md) for native transport libraries.
No printer is required for offline tests.

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

The build/check scripts reset generated `target/` artifacts on the first run
after 14 days. Initial use adopts an existing cache; later runs reuse it until
the next reset. Source, printer settings, and `local/` are retained. Direct
`cargo build` and CI have their own cache handling.

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
`printer/` owns job validation; `label.rs` and `image_encode.rs` own rendering.
[AGENTS.md](../AGENTS.md) gives exact boundaries and verification rules.

See [hardware notes](hardware-notes.md), [maintenance history](maintenance-notes.md),
and [0.32 API removals](../CHANGELOG.md#removed) for the relevant context.
