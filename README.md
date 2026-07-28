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

# Prefer the full BLE name from scan (macOS uses UUIDs, not MACs)
./target/release/thermark info -a "B1-YourPrinter"

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

## Tests

```bash
cargo test
```

## Protocol notes

See `AGENTS.md` for packet format, BLE UUIDs, error codes, and operational pitfalls.

Community references: the protocol notes in src/protocol.rs, the protocol reference, the simple print-task form.

## License

MIT
