# AGENTS.md — thermark

## Working contract

Carry the requested task through implementation, appropriate verification, and
any authorized delivery. Make routine implementation decisions from the task and
repository evidence; do not stop at a plan or first patch. Continue through fixes
and rerun affected checks without asking for permission at every step.

- Check `git status --short --branch` before editing and preserve unrelated work.
- Read the relevant code and tests, not every document. Use the reference map
  below to find the owner of a behavior; reuse context already gathered.
- Ask when missing information materially changes the outcome or an action falls
  outside the user's authorization. Continue independent work while waiting.
  Explicit user instructions take precedence over repository workflow defaults.
- A request to push includes committing, checking the remote, and a normal push
  once validation passes. Reuse authorization from the current task; do not ask
  again. Preserve concurrent changes and do not force-push shared history.
- Publishing releases, contacting people, and spending label media are separate
  actions: perform them when requested or authorized by the task, not as an
  automatic consequence of a code cleanup. Local builds, tests, and previews
  can proceed without additional approval.
- Work directly on small tasks. Use parallel agents only when requested and when
  independent work justifies the coordination cost. Hardware access is serial.
- Finish with what changed, the verification result, delivery status, and any
  material limit (especially missing hardware verification). Keep it concise.
  If blocked, name the exact blocker and what is needed to proceed.

## Product and boundaries

**thermark** is a local Rust CLI/library for monochrome stickers over BLE or USB
serial: guest Wi-Fi and URL QR labels first; text, inventory, badges, and line art
second. No vendor app or cloud service. Keep vendor names out of the product and
crate name; interoperability descriptions are fine.

The owned **B1 over BLE** is the only hardware-verified path. USB serial is
implemented and mock-tested. Other profiles remain experimental until exercised
on their physical printers. Do not add colour separation, colour raster
protocols, or multi-colour media support.

`src/profile.rs::PROFILES` owns model names, geometry, default tasks, verification
status, and evidence. Do not recreate a parallel support table in code. Printing
outside B1 + b1 task + BLE requires `--allow-experimental`; the library API is
unrestricted. Mock tests and successful builds do not establish hardware support.

## Invariants

- Physical geometry belongs to `PrinterProfile`; `PrintTask` owns wire behavior.
  Compose and validate through the connected client's profile so model, DPI,
  and effective width agree. Defaults come from `PrintTask::for_model`.
- `label::qr_layout` is the only owner of QR-beside-text geometry.
- `transport::name_looks_like_label_printer` owns printer-name heuristics.
  BLE selection is exact by default; substring matching requires `--fuzzy`.
  Require the printer GATT UUIDs, never a random characteristic fallback.
- `Pacing::INSTANT` differs from `Pacing::REAL` only in durations, not retries or
  control flow. Keep pacing validation, transport separation, safe printing,
  raw-printer access, and disconnect guarantees as explicit boundaries.
- The charged B1 can print the whole canvas. Its 1 mm top/bottom safe area is a
  registration margin, not a proven unprintable band. Check power and repeated
  output before changing geometry; use measured artifacts as evidence.
- Keep personal artwork, printer identity captures, and real Wi-Fi labels under
  gitignored `local/`. Public fixtures contain demo data only.

## Find the relevant context

Paths below are relative to the repository root. Load hardware notes only for
connection, protocol, rendering, or physical print work.

| Work | Owner / reference |
| --- | --- |
| Hardware limits, connection diagnosis, protocol omissions, printing examples | [Hardware notes](docs/hardware-notes.md) |
| Setup, user commands, RFID and consumables | [README](README.md) |
| Saved config and resolution | `src/config.rs` |
| Frames, commands, faults, task sequences | `src/packet.rs`, `src/protocol.rs`, `src/errors.rs`, `src/print_task.rs` |
| Device profiles and support evidence | `src/profile.rs` |
| BLE/serial connections and matching | `src/transport.rs`, `src/transport/`, `src/doctor.rs` |
| Jobs, queries, pacing, raw access, teardown | `src/printer/` |
| Dimensions, raster encoding, fonts, QR/text layout | `src/geometry.rs`, `src/image_encode.rs`, `src/font.rs`, `src/label.rs` |
| Wi-Fi payloads | `src/wifi.rs` |
| CLI arguments, command dispatch, sessions | `src/cli/args.rs`, `src/cli/commands/`, `src/cli/session.rs` |
| Entry point and advisory stderr | `src/main.rs`, `src/cli/tips.rs` |
| Job ordering and error tests without a printer | `src/mock.rs`, `tests/protocol_integration.rs` |
| CI and release packaging | `.github/workflows/rust.yml`, `.github/workflows/release.yml` |

## Verification

Use tests that exercise observable behavior. Reproduce bugs offline where
possible; avoid tests that merely restate implementation or encode unverified
hardware theories. CLI tests use temporary `THERMARK_CONFIG` files and remove
inherited `THERMARK_ADDR` in child commands. Do not mutate process-wide environment
or working directory in parallel tests; use explicit inputs or subprocess settings.

For Rust changes, run:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

For dependency, shared-library, or transport changes, also run:

```bash
cargo test --locked --lib --no-default-features
cargo test --locked --lib --no-default-features --features ble
cargo test --locked --lib --no-default-features --features serial
```

For CLI feature gating or transport dependency changes, build both binaries:

```bash
cargo build --locked --bin thermark --no-default-features --features ble
cargo build --locked --bin thermark --no-default-features --features serial
```

The full suite includes golden renders, fixture boundaries, and label placement.
For focused rendering iteration, use `--preview`,
`cargo test --locked --test golden`, `cargo test --locked --test label_placement`, or
`scripts/compare-render.sh <ref>`. Accept golden changes with `UPDATE_GOLDEN=1`
only after inspecting an intended visual change. Do not update baselines just to
make a failure pass. Physical printing is for hardware questions or confirming
a deliberate visual change, not routine regression checks.

Documentation-only edits need a diff and reference/command review, not a rebuild.
Workflow edits need workflow validation and relevant hosted checks. Once required
checks pass, repeat or broaden them only for new changes or unresolved evidence.
Review `git diff --check` and the final diff before committing.

## Maintenance and releases

Check current stable Rust and direct crate releases when upgrading dependencies.
Prefer compatible `cargo update` changes; take a major upgrade for a concrete
feature, fix, or meaningful deletion. Keep `Cargo.toml`'s `rust-version`,
`rust-toolchain.toml`, CI, and `Cargo.lock` compatible. Verify with `--locked`.

Simplify by deleting duplicate representations and passing typed values directly.
Extract abstractions when a rule has multiple real callers or one named owner
protects an invariant. File age or line count alone is not a reason to refactor.
Remove compatibility aliases only in a deliberate breaking release after
checking existing scripts.

`scripts/compare-render.sh <ref>` checks rendering preservation.
`cargo bench --bench image_pipeline` measures CPU-only medians; compare on the same
host. Measure peak RSS in separate processes to avoid allocator carry-over.
Optional coverage uses `cargo llvm-cov --workspace --summary-only`; on macOS set
`LLVM_COV` and `LLVM_PROFDATA` to the Homebrew LLVM tools when needed.

Release jobs package full and BLE-only binaries for Linux x86_64/ARM64 and macOS
Apple Silicon/Intel, with checksums and pinned actions. Manual workflow dispatch
builds downloadable artifacts without publishing. A pushed `v*` tag publishes
only when it matches the Cargo package version. A dependency update or ordinary
push does not by itself authorize a new release.
