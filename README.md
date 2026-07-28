# thermark

**Print real stickers from your terminal** — links, inventory tags, name badges, photos — on a pocket thermal label printer over **Bluetooth LE** (or USB). No vendor app, no cloud, no account.

Built for B1-class 50×30 mm labels (**384×240 px** at 8 px/mm). Hardware-tested on a real B1.

| Why people use it | What you get |
|-------------------|--------------|
| Share a URL on a package or laptop | QR + short text, one command |
| Label bins, cables, samples | Dense multi-line tags that still scan |
| Desk / event name stickers | Clean QR + identity lines |
| Photo stickers (1-bit thermal) | Centered fit, margins, dither |
| Automate / script | Plain CLI + Rust library, offline |

| Model | Task | Status |
|-------|------|--------|
| **B1** | `b1` | **Tested** |
| B21 / B18 | `b21v1` | Experimental (`--allow-experimental`) |
| D11 / D110 | `d110` | Experimental |
| other | `simple` | Experimental |

## Install / build

```bash
cargo build --release          # → target/release/thermark
cargo test
cargo test --lib --no-default-features
```

| Feature | Default | Enables |
|---------|---------|---------|
| `ble` | yes | Bluetooth LE |
| `serial` | yes | USB serial |

## Setup (once)

Quit any official label app (one BLE client at a time).

```bash
./target/release/thermark scan --save          # full device name → config
./target/release/thermark doctor --use-config  # lid / paper / battery
```

Config: macOS `~/Library/Application Support/thermark/config.json` · Linux `~/.config/thermark/config.json`  
Address priority: **`-a`** → **`THERMARK_ADDR`** → **config**. Match is **exact** name/id; **`--fuzzy`** only if you mean it.

---

## Sticker recipes

Always pass **`--label 50x30`** (or your media) so the canvas matches the physical sticker. After `scan --save`, omit `-a`.

### Link / package sticker

Scan on a phone → open the URL. Text is what humans read without scanning.

```bash
./target/release/thermark qr \
  --url "https://example.com/o/1042" \
  --text $'ORDER #1042\nShip by Fri\nPriority' \
  --font-name helvetica \
  --label 50x30
```

<img src="fixtures/sticker_link.png" alt="Link / order sticker" width="384" />

### Inventory / bin tag

Dense type for SKU, qty, date — still leaves a quiet margin so the printer edge doesn’t smear text.

```bash
./target/release/thermark qr \
  --url "https://example.com/bin/A3" \
  --text $'BIN A-3\nSKU 88421\nQTY 24\n2026-03' \
  --font-name helvetica \
  --font-size 12 \
  --label 50x30
```

<img src="fixtures/sticker_inventory.png" alt="Inventory bin tag" width="384" />

### Name / desk badge

```bash
./target/release/thermark qr \
  --url "https://example.com/u/ada" \
  --text $'ADA LOVELACE\nLab · Desk 12' \
  --font-name times \
  --label 50x30
```

<img src="fixtures/sticker_name.png" alt="Name badge sticker" width="384" />

### Calibrate print area

Full-bleed pattern to verify margins and feed before a batch.

```bash
./target/release/thermark calibrate --label 50x30
```

<img src="fixtures/sticker_calibrate.png" alt="Calibration sticker" width="384" />

### Photo sticker

Thermal is **1-bit**. Center the photo and dither so midtones don’t turn into black blobs:

```bash
./target/release/thermark print \
  -i fixtures/photo_sticker.jpg \
  --label 50x30 \
  --no-fill --margin 16 --dither -d 3
```

| Flag | Effect |
|------|--------|
| `--no-fill` | Whole image, centered (no crop) |
| `--margin N` | White inset (px) — avoids edge bleed |
| `--dither` | Floyd–Steinberg (photos) |
| `-d 3` | Normal density |

Preview QR without printing: `qr ... --save /tmp/out.png --no-print`.

```bash
./target/release/thermark fonts
./target/release/thermark tasks
```

---

## Geometry

| | |
|--|--|
| Resolution | ~**8 px/mm** (203 dpi) |
| Max width (B1) | **384 px** (~48 mm) |
| 50×30 mm sticker | **384×240 px** |

## Doctor

```bash
./target/release/thermark doctor                 # host + scan
./target/release/thermark doctor --use-config    # + connect / sensors
```

| Exit | Meaning |
|------|---------|
| **0** | Pass or warnings |
| **1** | FAIL — fix before printing |

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

## Fixtures

[`fixtures/`](fixtures/) holds the same stickers as this README. Locked by tests (size, ink distribution, encode, photo margins):

```bash
cargo test --test fixtures_readme
```

| File | Use |
|------|-----|
| `sticker_link.png` | Package / share URL |
| `sticker_inventory.png` | Bin / SKU tag |
| `sticker_name.png` | Name badge |
| `sticker_calibrate.png` | Geometry / bleed check |
| `photo_sticker.jpg` | Photo print source |

## Logging

```bash
./target/release/thermark -v info
RUST_LOG=thermark=debug ./target/release/thermark scan
```

## Protocol

See [`AGENTS.md`](AGENTS.md). Community: the protocol notes in src/protocol.rs.

## License

MIT
