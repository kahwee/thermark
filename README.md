# thermark

Local **thermal label printing** over **Bluetooth LE** or **USB serial** — no vendor desktop app required.

Print calibration patterns, PNG/JPEG rasters, and **QR + text** labels with system fonts (Helvetica, Times, Arial, …). Sized in real millimetres (8 px/mm).

Works with common pocket label printers that speak the reverse-engineered “B1-class” BLE protocol.

### Hardware support (honest)

| Model | Print task | Status in this repo |
|-------|------------|---------------------|
| **B1** | `b1` | **Tested** on real hardware (BLE print, QR, calibrate, info) |
| B21 / B18 | `b21v1` | Experimental (protocol docs only) |
| D11 / D110 | `d110` | Experimental (narrow head; not verified here) |
| other | `simple` | Experimental fallback |

```bash
./target/release/thermark tasks
# force sequence:
./target/release/thermark print ... --task b1
```

## Build

```bash
cargo build --release
# binary: target/release/thermark
```

### Cargo features

| Feature | Default | What it enables |
|---------|---------|-----------------|
| `ble` | yes | Bluetooth LE (`btleplug`; Linux needs `libdbus-1-dev`) |
| `serial` | yes | USB serial (`serialport`; Linux needs `libudev-dev`) |

Library-only (protocol + mock, no hardware deps):

```bash
cargo build --lib --no-default-features
cargo test --lib --no-default-features
```

The CLI binary requires both `ble` and `serial` (the default feature set).

## Usage

```bash
# Scan for printers
./target/release/thermark scan

# Save default BLE device (macOS: ~/Library/Application Support/thermark/config.toml)
./target/release/thermark config set -a "B1-YourPrinter"
./target/release/thermark config show

# Prefer the full BLE name from scan (macOS uses UUIDs, not MACs)
# After config set, -a is optional:
./target/release/thermark info
./target/release/thermark info -a "B1-YourPrinter"   # still works; overrides config

# Calibration — full label canvas
./target/release/thermark calibrate -a "B1-YourPrinter" --label 50x30 -d 4

# QR + text (Helvetica, small type)
./target/release/thermark qr -a "B1-YourPrinter" \
  --url "https://www.youtube.com" \
  --text $'ABC\nYOUTUBE' \
  --font-name helvetica \
  --font-size 14 \
  --label 50x30 -d 4

# Print an image
./target/release/thermark print -a "B1-YourPrinter" -i label.png --label 50x30 --fill

# List usable system fonts
./target/release/thermark fonts

# Diagnose host + printer (Bluetooth, scan, lid/paper/RFID)
./target/release/thermark doctor
./target/release/thermark doctor -a "B1-YourPrinter"
```

### Doctor (readiness checks)

Use `doctor` when something fails — offline printer, lid open, no paper, Bluetooth off.

```bash
# Host only (no -a): crate version, fonts, serial ports, Bluetooth adapter, BLE scan
./target/release/thermark doctor

# Full path: also connect + heartbeat (cover / paper / RFID / battery-ish)
./target/release/thermark doctor -a "B1-YourPrinter"
./target/release/thermark doctor -a "B1-YourPrinter" -s 8   # longer scan
./target/release/thermark doctor --use-config             # connect using saved addr
```

### Saved printer config

| | |
|--|--|
| File | macOS `~/Library/Application Support/thermark/config.toml` · Linux `~/.config/thermark/config.toml` |
| Set | `thermark config set -a "B1-YourPrinter"` |
| Show | `thermark config show` / `thermark config path` |
| Clear | `thermark config clear` |
| Env overrides | `THERMARK_ADDR` (addr only) · `THERMARK_CONFIG` (file path) |

Priority for address: **`-a` flag** → **`THERMARK_ADDR`** → **config file**.

| Exit code | Meaning |
|-----------|---------|
| **0** | Overall pass or warnings only |
| **1** | At least one **FAIL** (fix before printing) |

Typical offline printer output:

- `[ok] bluetooth` — adapter is up  
- `[FAIL] ble_scan` — no devices in N seconds (power on printer, quit vendor apps)  
- With `-a` but printer off: `[FAIL] ble_connect` — nothing matching that name  

When connected:

- `[FAIL] cover` — lid open  
- `[FAIL] paper` — no labels detected  
- `[ok] rfid` / paper counters from the tag  

Quit any official label app before connecting (one BLE client at a time).

## Geometry

| | |
|--|--|
| Resolution | ~**8 px/mm** (203 dpi) |
| Max print width (B1-class) | **384 px** (~48 mm) |
| Example 50×30 mm label | **384×240 px** |

Always pass `--label WWxHH` so the canvas matches your media.

## Library

```rust
use thermark::{BleTransport, Model, PrinterClient};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ble = BleTransport::connect("B1-YourPrinter", Duration::from_secs(5)).await?;
    let mut p = PrinterClient::new(ble, Model::B1);
    println!("{}", p.fetch_summary().await?);
    Ok(())
}
```

## Logging

Uses [`tracing`](https://docs.rs/tracing). Defaults to `info`; `-v` → `debug`. Override with `RUST_LOG`:

```bash
RUST_LOG=thermark=debug,btleplug=info ./target/release/thermark scan
./target/release/thermark -v info -a "B1-YourPrinter"
```

## Tests

```bash
cargo test
```

## Protocol notes

See `AGENTS.md` for packet format, BLE UUIDs, error codes, and operational pitfalls.

Community references: the protocol notes in src/protocol.rs, the protocol reference, the simple print-task form.

## License

MIT
