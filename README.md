# thermark

Local, scriptable sticker printing for pocket thermal printers over Bluetooth LE
or USB serial. No vendor app, cloud service, or account.

thermark is intentionally **monochrome** and **B1-first**. The primary tested
setup is a B1-class printer with 50×30 mm labels rendered at 384×240 px.

## What it does

- Guest Wi-Fi stickers: network name plus a QR code to join.
- URL stickers: a scannable link with human-readable text.
- Plain text, inventory, badge, and line-art labels.
- Exact physical sizing from a CLI or Rust program.
- Local previews before using paper.

## Support

| Model family | Status |
|---|---|
| B1 | Hardware-tested |
| B1 Pro, B21 Pro, D11, D11_H, D110 | Experimental monochrome profiles |
| B18 | Geometry known; print task unresolved |

Experimental models require `--allow-experimental`. Multi-colour printheads and
colour raster protocols are out of scope.

Requires Rust 1.97 or newer. [`rust-toolchain.toml`](rust-toolchain.toml) tracks
the current stable toolchain for rustup users.

## Build and set up

```bash
cargo build --release

# Quit the vendor app first; only one BLE client can hold the printer.
./target/release/thermark scan --save
./target/release/thermark identify
./target/release/thermark doctor --use-config
```

For maintenance, keep the lockfile current and run the same checks as CI:

```bash
cargo update --dry-run
cargo update
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
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

## Print stickers

Always pass the physical label size. After `scan --save`, the printer address is
read from config.

Guest Wi-Fi:

```bash
./target/release/thermark wifi \
  --ssid "YourNetwork" \
  --password 'your-password' \
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

## Preview and calibrate

Preview the exact bitmap without printing:

```bash
./target/release/thermark print \
  -i local/prints/art.png \
  --label 50x30 \
  --preview local/preview.png
```

Generated sticker commands use `--save <path> --no-print` for the same purpose.

Check label placement on hardware:

```bash
./target/release/thermark calibrate --label 50x30
./target/release/thermark calibrate --boundary --label 50x30
```

The printer does not report label dimensions, so `--label WxH` remains required
when media changes.

## If a print looks wrong

1. Run `thermark info` and charge the printer if the battery is low.
2. Preview the bitmap to separate rendering problems from hardware problems.
3. Print the same bitmap twice. Inconsistent truncation points indicate power,
   not geometry.
4. Use `calibrate --boundary` only after the printer is charged.

A charged B1 reaches the full feed canvas. Its default 1 mm top/bottom inset is
registration margin, not a known unprintable band.

For connection failures, quit the vendor app and copy the complete name from
`thermark scan`.

## Development

```bash
cargo test
cargo test --lib --no-default-features
cargo test --test golden
cargo test --test label_placement
cargo test --test fixtures_readme
```

The architecture keeps four concerns separate:

- `profile.rs`: detected printer identity and physical capabilities.
- `transport/`: BLE and USB communication.
- `printer/`: validated job lifecycle and protocol queries.
- `label.rs` / `image_encode.rs`: layout and monochrome raster generation.

See [`AGENTS.md`](AGENTS.md) for protocol details, hardware measurements, and
contributor invariants.

## License

MIT
