# AGENTS.md — thermark

Guidance for coding agents working in this repository.

## What this project is

**`thermark`** — local, scriptable **sticker printing** for pocket thermal printers over **Bluetooth LE** (and USB serial), without vendor apps or cloud.

Positioning: **terminal-first stickers** — especially **guest Wi‑Fi join QR** (SSID on label, password in QR) and package URL QRs — plus inventory, badges, line-art. Offline, exact mm geometry.

- Crate / binary: **`thermark`**
- Do **not** put vendor brand names in the package or product name
- Not AirPrint / CUPS — custom binary protocol (B1-class)

### Hardware exercised in development

| Item | Value |
|------|--------|
| Class | Pocket thermal, B1-class |
| Example BLE name | `B1-YourPrinter` (use full name) |
| Common label | **50×30 mm** → **384×240 px** |

macOS CoreBluetooth uses **UUID** device ids, not classic MACs.

---

## Commands

```bash
cargo build --release
cargo test
cargo test --lib --no-default-features
cargo test --test fixtures_readme   # sticker fixtures + boundary checks

# One-time setup (full BLE advertising name → config.json)
./target/release/thermark scan --save
./target/release/thermark doctor --use-config

# Stickers (fixtures/ + README). Canonical art smoke: sticker_turtle.png
./target/release/thermark print -i fixtures/sticker_turtle.png \
  --label 50x30 --no-fill --margin 0 -d 4
./target/release/thermark qr --url "https://example.com/o/1042" \
  --text $'ORDER #1042\nShip by Fri\nPriority' --font-name helvetica --label 50x30
./target/release/thermark calibrate --label 50x30
```

Fixtures locked by `tests/fixtures_readme.rs`. **Primary print/art smoke:** `fixtures/sticker_turtle.png`.
Quit vendor apps before BLE connect. BLE `-a` is **exact** by default (`--fuzzy` optional).  
Experimental print tasks need `--allow-experimental`.

---

## Module map

| File | Role |
|------|------|
| `config.rs` | User `config.json` (default BLE addr) |
| `packet.rs` | `55 55 \| CMD \| LEN \| DATA \| XOR \| AA AA` |
| `protocol.rs` | Commands, B1 PrintStart / page size, models |
| `errors.rs` | Print error 0xDB reason codes |
| `transport.rs` | BLE + serial; `PRINTER_SERVICE` / `PRINTER_CHAR` |
| `printer/` | Client + print job; `info` (heartbeat/RFID/summary) |
| `geometry.rs` | 8 px/mm, `LabelMm` / `LabelPx` |
| `image_encode.rs` | Raster → row packets |
| `font.rs` | System TTF/TTC (`ab_glyph`), named fonts |
| `label.rs` | Square QR + side text |
| `main.rs` | CLI |

---

## Protocol essentials

- BLE service `e7810a71-73ae-499d-8c15-faa9aef0c3f2`
- Characteristic `bef8d6c9-9c21-4c9e-b632-bd58c1009f9f`
- Write without response; notifications for replies
- B1: 7-byte PrintStart; 6-byte SetPageSize (rows, cols, copies)
- rows = image height (feed); cols = width ≤ 384
- `0xDB` first byte = `PrinterErrorCode` (0x01 cover, 0x02 no paper, …)
- Info response cmd = `0x40 + key`

Wiki: the protocol notes in src/protocol.rs

---

## Geometry & fonts

- **8 px/mm**; max width **384**
- Always use `--label` for full-size media
- Prefer system fonts (`--font-name helvetica|times|arial`) over bitmap fallback
- Default no decorative border; QR is square beside text column
- `--font-size N` for fixed small/large type; omit for auto-fit

---

## Pitfalls

1. BLE address match is **exact** by default (full advertising name or id). Short selectors no longer substring-match; use full name from `scan`, or pass `--fuzzy` only if intentional  
2. Require real printer GATT UUID; no random characteristic fallback  
3. Stuck CLI holds BLE lock  
4. Tiny prints = missing `--label` / wrong canvas, not “broken printer”  
5. Bitmap 5×7 font had mirrored text bugs — do not use for user labels  

---

## Print tasks

`PrintTask` in `src/print_task.rs` selects on-wire sequence:

| Task | Hardware-tested here? |
|------|------------------------|
| `B1` | **Yes** |
| `B21V1`, `D110`, `Simple` | No (experimental) |

`PrinterClient` defaults via `PrintTask::for_model`. Override: `--task` / `.with_print_task()`.

CLI: experimental tasks (`b21v1`, `d110`, `simple`, or models that map to them) require `--allow-experimental` on `print` / `qr` / `calibrate`. Library API is unrestricted.

## Tests

```bash
cargo test
```

- Pure logic: packets, geometry, layout, fonts
- **Mock transport** (`src/mock.rs`): full print job command order, 0xDB errors, summary
- Live BLE print remains manual

```bash
# coverage (needs Homebrew llvm + cargo-llvm-cov)
export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
cargo llvm-cov --workspace --summary-only
```

---

## Naming

Product/crate: **thermark**. Interoperability language in docs is fine (“B1-class printers”); avoid vendor branding in crate name.
