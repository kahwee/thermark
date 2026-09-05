# thermark

Local, scriptable sticker printing for pocket thermal printers over Bluetooth LE
or USB serial. No vendor app, cloud service, or account.

thermark is intentionally **monochrome** and **B1-first**. The primary tested
setup is a B1-class printer with 50×30 mm labels rendered at 384×240 px.
The B1-over-BLE path is the only hardware-verified transport in this repository.
USB serial is implemented and mock-tested, but has not been verified against
the owned printer.

## What it does

- Guest Wi-Fi stickers: network name plus a QR code to join.
- URL stickers: a scannable link with human-readable text.
- Plain text, inventory, badge, and line-art labels.
- Exact physical sizing from a CLI or Rust program.
- Local previews before using paper.

## Support

| Model family / path | Status |
|---|---|
| B1 over BLE | Hardware-tested |
| B1 over USB serial | Experimental; implemented and mock-tested |
| B1 Pro, B21 Pro, D11, D11_H, D110 | Experimental monochrome profiles |
| B18 | Geometry known; print task unresolved |

Printing with any profile/task/connection combination other than B1+B1 over
BLE requires `--allow-experimental`; offline previews and saved renders do not.
Multi-colour printheads and colour raster protocols are out of scope.

Requires Rust 1.98 or newer. [`rust-toolchain.toml`](rust-toolchain.toml) tracks
the current stable toolchain for rustup users.

## Build and set up

```bash
cargo build --release

# Quit the vendor app first; only one BLE client can hold the printer.
./target/release/thermark scan --save
./target/release/thermark identify
./target/release/thermark doctor --use-config
```

For the smallest primary-path binary, omit USB serial support and build BLE
only:

```bash
cargo build --locked --release --no-default-features --features ble
```

Tagged GitHub releases publish both `full` (BLE + USB serial) and `ble`
archives for Linux x86_64/ARM64 and macOS Apple Silicon/Intel, with SHA-256
checksum files. Linux binaries are built on Ubuntu 24.04 and macOS binaries on
macOS 15. The release workflow also supports a manual branch run for testing
downloadable artifacts without creating a release. A pushed release tag must
exactly match the Cargo version, such as `v0.32.0` for package version
`0.32.0`.

Archive names include the package version and platform, for example
`thermark-0.32.0-macOS-ARM64-ble.tar.gz`. After downloading the archive and its
`.sha256` sidecar from GitHub Releases, verify and unpack it:

```bash
# Linux
sha256sum --check thermark-0.32.0-Linux-X64-ble.tar.gz.sha256

# macOS
shasum -a 256 --check thermark-0.32.0-macOS-ARM64-ble.tar.gz.sha256

tar -xzf thermark-0.32.0-macOS-ARM64-ble.tar.gz
cd thermark-0.32.0-macOS-ARM64-ble
./thermark --version
./thermark tasks
```

Refresh compatible dependency versions deliberately, then review the lockfile
diff:

```bash
cargo update --dry-run
cargo update
```

Before pushing, run the validation and feature matrix used by CI:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo test --locked --lib --no-default-features
cargo test --locked --lib --no-default-features --features ble
cargo test --locked --lib --no-default-features --features serial
cargo build --locked --bin thermark --no-default-features --features ble
cargo build --locked --bin thermark --no-default-features --features serial
```

Direct dependencies intentionally use compatible major-version ranges while
`Cargo.lock` pins reproducible builds. A major-version bump should earn its
complexity with a concrete feature, fix, or code deletion; newer by itself is
not enough.

To capture the exact model, firmware, geometry, and task for a hardware report:

```bash
./target/release/thermark identify --json \
  > local/printer-identity.json
```

Use the full advertising name shown by `scan`. Matching is exact by default;
`--fuzzy` enables intentional substring matching. On macOS, Bluetooth device IDs
are UUIDs rather than classic MAC addresses.

### macOS Bluetooth ownership

macOS can show a printer as **Connected** while thermark reports that it is not
discoverable. One likely cause is that another Bluetooth client—often a vendor
app or a system-managed session—still owns this printer's BLE GATT connection.
The same symptom can also mean the printer is asleep, out of range, or not
advertising, so treat ownership as a diagnosis to verify rather than a fact.

Before retrying, disconnect the printer in macOS Bluetooth settings, quit any
vendor label app, wake or power-cycle the printer, and then run:

```bash
./target/release/thermark scan
./target/release/thermark doctor --use-config
```

If `doctor` reports a matching `/dev/cu.…` endpoint, that supports the ownership
diagnosis, but proves neither ownership nor serial-protocol compatibility. Use
the endpoint with `--conn usb` only when the printer responds to the serial
protocol; otherwise release any competing Bluetooth session and use BLE
normally.

## Print stickers

Always pass the physical label size. After `scan --save`, the printer address is
read from config.

Guest Wi-Fi:

```bash
THERMARK_WIFI_PASSWORD='your-password' \
  ./target/release/thermark wifi \
  --ssid "YourNetwork" \
  --label 50x30
```

Open networks do not need a password:

```bash
./target/release/thermark wifi \
  --ssid "Cafe-Guest" \
  --security nopass \
  --label 50x30
```

URL with readable text:

```bash
./target/release/thermark qr \
  --url "https://example.com/o/1042" \
  --text $'ORDER #1042\nPriority' \
  --font-name helvetica \
  --label 50x30
```

Plain text:

```bash
./target/release/thermark text \
  --text $'FRAGILE\nthis way up' \
  --label 50x30
```

Existing artwork:

```bash
./target/release/thermark print \
  -i local/prints/art.png \
  --label 50x30 \
  --no-fill \
  --margin 0 \
  -d 4
```

Personal artwork and real credentials belong under `local/`, which is ignored
by git. Committed files under [`fixtures/`](fixtures/) contain public demo data
only.

### Which printer profile controls rendering

For an online `text`, `qr`, `wifi`, or `calibrate` command, thermark connects
and identifies the printer before converting the physical label size to pixels.
The detected profile's DPI and printhead width therefore determine the rendered
canvas. If a generated-label command also uses `--save`, the PNG is written
from that same detected-profile render before it is printed.

Hardware printing fails closed if the identity probe fails or reports an
unrecognized model. thermark will not render or send a job using provisional
profile geometry in that case.

`--no-print` remains connection-free. In that mode, generated labels use the
profile selected by `--model`, then the saved configuration, then the B1
default. Combine `--save <path> --no-print` when you want a wholly offline
render.

Online raw-image printing also lays out the image against the detected profile.
Its default 1 mm registration margin is converted at the detected DPI; an
explicitly saved pixel inset and `--full-bleed` remain exact.

## Preview and calibrate

Preview the exact bitmap for the selected profile without printing:

```bash
./target/release/thermark print \
  -i local/prints/art.png \
  --label 50x30 \
  --preview local/preview.png
```

`print --preview` applies the selected threshold and dithering and writes the
final monochrome page for the configured or `--model` profile. If that profile
does not match hardware later detected by an online print, its geometry can
differ. Generated sticker commands use
`--save <path> --no-print` to inspect their composed artwork; those saved PNGs
can retain antialiasing that the printer later thresholds. Without
`--no-print`, `--save` waits until printer identification so the saved bitmap
and printed page share the same profile-sized render.

Check label placement on hardware:

```bash
./target/release/thermark calibrate --label 50x30
./target/release/thermark calibrate --boundary --label 50x30
```

### Label size and RFID

The printer's RFID response can describe the consumable type and remaining
label count, but it does not report dimensions in millimetres. Vendor software
may resolve an RFID barcode through its own catalogue; thermark has no cloud
catalogue. Pass `--label WxH` when media changes, or save the size with
`thermark config set --label WxH`.

## If a print looks wrong

1. Run `thermark info` and charge the printer if the battery is low.
2. Preview the bitmap to separate rendering problems from hardware problems.
3. Print the same bitmap twice. Inconsistent truncation points indicate power,
   not geometry.
4. Use `calibrate --boundary` only after the printer is charged.

A charged B1 reaches the full feed canvas. Its default 1 mm top/bottom inset is
registration margin, not a known unprintable band.

For connection failures on macOS, follow the
[Bluetooth ownership checks](#macos-bluetooth-ownership) above.

## Development

```bash
cargo test
cargo test --lib --no-default-features
cargo test --test golden
cargo test --test label_placement
cargo test --test fixtures_readme
cargo bench --bench image_pipeline
```

The benchmark reports CPU-only medians. Compare runs on the same host, and
measure peak RSS in separate processes when evaluating memory changes.

Representative same-host Apple Silicon measurements for 0.32.0:

| Optimization | Before | After |
|---|---:|---:|
| 2000×1500 RGB trim scratch storage | 3,000,000 B | at most 512,000 B |
| 2000×1500 RGB trim median | 7.11 ms | 7.20 ms |
| macOS ARM64 release binary | 3,670,016 B full | 3,601,600 B BLE-only |

The RGB change removes 82.9% of temporary scan storage while keeping runtime
effectively flat. Protocol sends also avoid one heap allocation per packet;
exhaustive tests compare the fixed-buffer and public allocating encoders for
all payload lengths from 0 through 255. These figures are evidence from one
machine, not cross-platform performance guarantees.

The architecture keeps four concerns separate:

- `profile.rs`: detected printer identity, physical capabilities, default task,
  and the single support-status registry exposed as `PROFILES`.
- `transport/`: BLE and USB communication.
- `printer/`: validated job lifecycle and protocol queries.
- `label.rs` / `image_encode.rs`: layout and monochrome raster generation.

For library API migrations, see the explicit old-to-new mappings under
[0.32.0 → Removed](CHANGELOG.md#removed).

See [`AGENTS.md`](AGENTS.md) for protocol details, hardware measurements, and
contributor invariants.

## License

MIT
