# thermark agent guide

thermark is a local Rust CLI/library for monochrome labels over BLE or USB
serial. B1 over BLE is the only hardware-verified path. USB is mock-tested;
other profiles are experimental. Do not treat a build or mock test as hardware
verification, or add colour printing. Preserve unrelated working-tree changes.

## Product boundaries

- `src/profile.rs::PROFILES` is the single registry for model geometry, tasks,
  verification status, and evidence. Printing outside B1 + b1 task + BLE
  requires `--allow-experimental`; offline rendering does not.
- `PrinterProfile` owns physical geometry; `PrintTask` owns wire behavior. The
  connected profile determines render dimensions. Defaults come from
  `PrintTask::for_model`.
- `label::qr_layout` owns QR-beside-text geometry.
  `transport::name_looks_like_label_printer` owns name matching. BLE matching
  is exact unless `--fuzzy` is supplied, and requires the printer GATT UUIDs.
- `Pacing::INSTANT` may change durations, not retries or control flow. Preserve
  transport separation, safe printing, raw access, and disconnect guarantees.
- The B1's 1 mm top/bottom safe area is a registration margin, not a proven
  unprintable band. Check power and repeated output before changing geometry.
- Keep personal artwork, printer identities, and real Wi-Fi labels in ignored
  `local/`; public fixtures must use demo data.

## Find the owner

Read only references relevant to the task. [Hardware notes](docs/hardware-notes.md)
cover connection, protocol, rendering, and physical measurements. The
[README](README.md) covers user commands and consumables.

| Area | Files |
| --- | --- |
| Profiles and tasks | `src/profile.rs`, `src/print_task.rs` |
| Protocol and errors | `src/packet.rs`, `src/protocol.rs`, `src/errors.rs` |
| Connections and diagnosis | `src/transport.rs`, `src/transport/`, `src/doctor.rs` |
| Jobs and queries | `src/printer/` |
| Rendering | `src/geometry.rs`, `src/image_encode.rs`, `src/font.rs`, `src/label.rs` |
| CLI and config | `src/cli/`, `src/config.rs`, `src/main.rs` |
| Offline integration tests | `src/mock.rs`, `tests/protocol_integration.rs` |

## Verify

Use `scripts/check.sh` for Rust changes; use `scripts/check.sh all` for
dependencies, transports, feature gating, or shared APIs. `scripts/check.sh
render` runs focused image checks. Documentation-only changes need diff and
link/command review. The script rejects inherited `UPDATE_GOLDEN`. Inspect
`target/golden-actual/` before accepting a golden update with
`UPDATE_GOLDEN=1`; never update baselines just to pass a test. Physical prints
answer hardware questions, not routine regressions.

CLI tests should use temporary `THERMARK_CONFIG` and unset inherited
`THERMARK_ADDR` in child commands. Avoid process-wide environment or cwd
mutation in parallel tests. Keep `Cargo.toml`'s `rust-version`,
`rust-toolchain.toml`, CI, and `Cargo.lock` compatible; verify with `--locked`.
For release behavior, read `.github/workflows/release.yml` and the hardware
notes. A routine push or dependency update does not authorize a release.
