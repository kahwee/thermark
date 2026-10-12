# Development

Requires Rust 1.99+ and the [native prerequisites](installation.md). Rustup uses
`rust-toolchain.toml`. Offline tests need no printer.

## Build and check

```sh
cargo build --locked
scripts/check.sh          # formatting, Clippy, tests and docs
scripts/check.sh all      # also BLE-only, serial-only and no transports
scripts/check.sh render   # focused image checks
cargo run --locked --no-default-features --example prepared_job
python3 scripts/test_hardware_trial.py -v
```

Use `all` for dependencies, transports and shared APIs. The check script rejects
inherited `UPDATE_GOLDEN`; inspect `target/golden-actual/` before accepting a
baseline. [CONTRIBUTING.md](../CONTRIBUTING.md) covers test isolation;
[architecture](architecture.md) maps module ownership. Follow the
[release checklist](releasing.md) before tagging.

## Documentation images and screenshots

README, recipes and the landing page share PNGs under `docs/demo/`. Refresh them
from the current checkout, then verify them:

```sh
python3 scripts/render-docs.py
python3 scripts/render-docs.py --check
```

The script builds a locked, transport-free CLI, uses an isolated configuration
and embedded DejaVu Sans, and stages all nine images before replacing files.
`manifest.json` records the build, font hash, dimensions setting and image hashes.
CI checks for stale images. Review each PNG at native size for clipping and QR
legibility; `--check` proves reproducibility, not physical print quality.

These images are rendered labels. Capture the actual landing page separately:

```sh
# Optional dependency; use system Chromium or install Playwright's browser:
python3 -m pip install playwright
python3 -m playwright install chromium
python3 scripts/capture-docs.py --output local/screenshots/review-1
```

The capture script serves local docs, checks image loading/overflow and saves full-page
screenshots at 1440×1000 and 390×844 with the walkthrough stopped. It records
browser/version, viewport and content hashes. Use a fresh output directory;
review screenshots stay under ignored `local/`. Physical-print photos should
show charged-printer output laid flat using demo data.

`fixtures/` and `tests/golden/` are separate regression baselines. The refresh
script never updates them; review renderer changes before accepting baselines.
For your own gallery, run `sh examples/render-recipes.sh local/prints/recipes`.

## Benchmarks

```sh
cargo bench --locked --bench image_pipeline
cargo bench --locked --no-default-features --bench packet_decoder
PROPTEST_CASES=4096 cargo test --locked --no-default-features --test packet_stream
```

Compare revisions on the same host without concurrent builds; measure peak RSS
in separate processes. Packet properties cover fragmentation, corruption and
bounded retention. Commit regression seeds when fixing failures.

Historical measurements explain the current design, not performance guarantees:

| Measurement | Result and context |
| --- | --- |
| Decoder cursor/compaction | macOS ARM64, Rust 1.98.1, 2026-09-25: median of 31 samples for 8,192 frames with 48-byte payloads. One bulk read: 39.515→0.246 ms; 20-byte reads: 0.626→0.681 ms. Retained input is bounded; returned packets scale with output. |
| Disk-backed preparation | Linux x86_64, Rust 1.99, v0.35.0: 1/750 text labels took 0.006/0.346 s; child peak RSS was 9,856 KiB for both, including the launcher. |
| Shared font/layout cache | Same Linux host: median text render 0.075 ms fresh versus 0.048 ms shared; Arabic shared 0.028 ms. |

No physical printer throughput or competitor benchmark is established by these
results. Use the [hardware trial](hardware-trial.md) for physical measurements.

## Dependency security and caches

```sh
cargo install cargo-audit --version 0.22.2 --locked
cargo audit --deny unsound --deny yanked
```

CI audits dependency changes and runs weekly. Vulnerabilities, unsoundness and
yanked crates fail; unmaintained warnings remain visible. `ab_glyph` depends on
`ttf-parser` ([RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192));
there is no patched version. Font-engine changes need rendering checks.

Builds reuse artifacts. `scripts/prune-build-cache.sh` removes this checkout's
`target/` on explicit request; custom target directories and CI caches are separate.
