# AGENTS.md — thermark

Guidance for coding agents working in this repository.

## What this project is

**`thermark`** — local, scriptable **sticker printing** for pocket thermal printers over **Bluetooth LE** (and USB serial), without vendor apps or cloud.

**Problem:** pocket thermals are stuck behind vendor apps; you can’t easily print guest Wi‑Fi / URL stickers offline with exact mm size from a CLI.

**Positioning:** local tool for stickers on real objects — **scan to join (Wi‑Fi)** and **scan to open (URL)** first; inventory, badges, line-art second. Offline, scriptable, no cloud.

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

# Stickers (fixtures/ product demos; personal art → local/prints/)
./target/release/thermark print -i fixtures/sticker_wifi.png \
  --label 50x30 --no-fill --margin 0 -d 4
./target/release/thermark qr --url "https://example.com/o/1042" \
  --text $'ORDER #1042\nShip by Fri\nPriority' --font-name helvetica --label 50x30
./target/release/thermark text --text $'FRAGILE\nthis way up' --label 50x30
./target/release/thermark calibrate --label 50x30   # rings + safe-area box
```

Artwork demo: `cargo run --example bulldozer -- local/prints/bulldozer.png`

Check placement without a printer: `thermark print -i art.png --label 50x30 --preview out.png`

Fixtures locked by `tests/fixtures_readme.rs` (wifi, link, inventory, name, calibrate only).
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
| `geometry.rs` | 8 px/mm, `LabelMm` / `LabelPx`, `HEAD_*_PX` widths |
| `image_encode.rs` | Image → `Raster` (rows + dimensions together) |
| `font.rs` | System TTF/TTC (`ab_glyph`), named fonts |
| `label.rs` | Square QR + side text; `qr_layout` owns the geometry |
| `main.rs` | Entry point only (parse → dispatch → exit) |
| `cli/args.rs` | clap types + shared arg groups (`ConnArgs`, `TaskArgs`, `FontArgs`) |
| `cli/session.rs` | Connect → print → disconnect; `resolve_task` |
| `cli/commands/` | One module per command group |
| `cli/tips.rs` | Advisory stderr only; never changes behaviour |

### Invariants worth keeping

- **Widths:** size canvases and check rasters with
  `print_task::effective_max_width_px(model, task)` — never `Model::max_width_px`
  alone, or a mismatched `--model`/`--task` encodes before failing.
- **Layout:** QR-beside-text geometry lives only in `label::qr_layout`.
- **Printer names:** the "looks like a label printer" heuristic lives only in
  `transport::name_looks_like_label_printer`.
- **Pacing:** tests use `Pacing::INSTANT`, which differs from `Pacing::REAL`
  only in durations — never in retry counts or control flow.

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
- Printable window on B1 + 50x30 is **48 x 25 mm** of a 50 x 30 label,
  confirmed against hardware (the safe-area box prints complete on all four
  sides). The remaining white is physical: ~4.6 mm at the feed edge, and 2 mm
  across because the head is 48 mm. If the left/right white is lopsided the
  roll is off-centre in the guide — not a software problem. Older note:
  measured with `thermark calibrate`. The printer starts a little after the
  leading edge and stops before the trailing edge; rows past the window are
  dropped, not scaled. `SafeArea::B1` = top 0 / bottom 40 / left 0 / right 0.
  The residual white at the feed edges (~2 mm) and across (48 mm head on a
  50 mm label) is physical — do not try to fix it in software
- Always use `--label` for full-size media
- Prefer system fonts (`--font-name helvetica|times|arial`) over bitmap fallback
- Default no decorative border; QR is square beside text column
- `--font-size N` for fixed small/large type; omit for auto-fit

---

## Pitfalls

1. BLE address match is **exact** by default (full advertising name or id). Short selectors no longer substring-match; use full name from `scan`, or pass `--fuzzy` only if intentional  
2. Require real printer GATT UUID; no random characteristic fallback  
3. Stuck CLI holds BLE lock — mostly addressed: Ctrl-C cancels the job and
   `BleTransport::drop` blocks until disconnect on a multi-threaded runtime.
   `SIGKILL` still leaves the link held until the printer times out  
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
