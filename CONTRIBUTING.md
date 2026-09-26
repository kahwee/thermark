# Contributing to thermark

The most useful contribution is a reproducible report from a printer someone
actually owns. B1 over BLE is the only hardware-tested path today.

## Test another printer

1. Follow the [installation guide](docs/installation.md) for Homebrew, Cargo, or release downloads.
2. Run `thermark --version`, `thermark scan`, and `thermark doctor --json`.
3. Choose the correct model and actual label dimensions. Experimental paths
   require `--allow-experimental`; review `thermark tasks` before printing.
4. Print public demo content, check its QR with a phone, and repeat once. Note
   missing rows, alignment, density, and whether the printer feeds correctly.
5. Open a [compatibility report](https://github.com/kahwee/thermark/issues/new?template=hardware-report.yml).

Use fake Wi-Fi credentials and example.com URLs. Review screenshots and logs;
never publish device addresses, serial numbers, RFID IDs, or actual passwords.
`doctor --json` is designed for public reports. Raw debug logs and
`identify --json` are not privacy-safe replacements.

## Code and documentation

Start with a checkout and the locked dependency graph:

```sh
git clone https://github.com/kahwee/thermark.git
cd thermark
cargo build --locked
scripts/check.sh
```

For system prerequisites, use the [installation guide](docs/installation.md).
No printer is required to build or run the offline tests.

Read [AGENTS.md](AGENTS.md) for module ownership and product boundaries.
Run `scripts/check.sh all` for dependency, transport, or shared API changes;
`scripts/check.sh` is sufficient for other Rust changes. Rendering baselines
must be visually reviewed before acceptance. Tests and mock transports do not
establish hardware verification.

Small fixes, clearer setup instructions, reproducible bugs, and reports of
accessibility or installation problems are welcome. Keep personal data in
ignored `local/`. Mention the OS, thermark version, and a minimal example.

## Where a change belongs

Keep parsing and presentation in `src/cli/`; put reusable behavior in the
library. `profile` owns physical dimensions and support evidence, `print_task`
owns job sequencing, and `packet` owns framing. Rendering stays independent of
BLE/serial transport. Prefer small private helpers within those owners over a
new catch-all utility module or abstraction with only one caller.

Use typed options and `Result` for caller-controlled failures. Propagate errors
with context at CLI/I/O boundaries. Borrow inputs when ownership is unnecessary;
do not add clones or asynchronous functions just to satisfy a call shape. Keep
the public API stable unless an intentional release documents the change.

## Choose tests by behavior

| Change | Useful evidence |
| --- | --- |
| Install instructions or first-run flow | CLI command in a temporary directory, readable PNG, decoded QR payload, no saved config |
| Packet framing | Known wire bytes, all payload lengths, corrupt input and fragmented streams |
| Print sequencing | Mock transport assertions for ordering, retries, errors, and disconnect |
| Geometry or image output | Placement tests and visually reviewed golden images |
| New model verification | Actual repeated physical prints; tests alone are insufficient |

The `installation_preview_is_scannable_without_creating_config` test in
`tests/cli.rs` covers the documented offline preview with a vendored font. It
runs with default, BLE-only, serial-only, and no transport features through
`scripts/check.sh all`. It does not install a package or access hardware.

CLI tests must set a temporary `THERMARK_CONFIG`, remove inherited
`THERMARK_ADDR`, and pass fonts explicitly when rendering must be reproducible.
Set environment variables and working directories on the child command, not
the test process. Assert user-visible results instead of duplicating the
implementation in the test.
