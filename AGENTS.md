# AGENTS.md — thermark

Guidance for coding agents working in this repository.

## What this project is

**`thermark`** — local, scriptable **sticker printing** for pocket thermal printers over **Bluetooth LE** (and USB serial), without vendor apps or cloud.

**Problem:** pocket thermals are stuck behind vendor apps; you can’t easily print guest Wi‑Fi / URL stickers offline with exact mm size from a CLI.

**Positioning:** local tool for stickers on real objects — **scan to join (Wi‑Fi)** and **scan to open (URL)** first; inventory, badges, line-art second. Offline, scriptable, no cloud.

**Scope:** monochrome direct-thermal output only. Do not add colour-layer
separation, compressed colour raster protocols, or multi-colour media support.
The owned B1 is the primary product path and the only path considered
hardware-verified; all other monochrome profiles stay experimental until run
on the corresponding physical printer.

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
The owned B1-over-BLE path is hardware-verified. USB serial is implemented and
mock-tested, but has not been verified against the owned printer.

---

## Commands

```bash
cargo build --release
cargo test
cargo test --lib --no-default-features
cargo test --test fixtures_readme   # sticker fixtures + boundary checks

# One-time setup (full BLE advertising name → config.json)
./target/release/thermark scan --save
./target/release/thermark identify --json > local/printer-identity.json
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

On macOS, Bluetooth Settings may show the printer as **Connected** while
CoreBluetooth cannot discover it. Treat that as an exclusive-session conflict:
another client owns the printer's GATT connection. `thermark doctor
--use-config` reports a matching `/dev/cu.…` endpoint as supporting evidence;
disconnect the printer in macOS Bluetooth settings, quit vendor apps, and
power-cycle or wake the printer before retrying BLE. A serial endpoint alone is
not proof that the printer accepts thermark's serial protocol.

---

## Module map

| File | Role |
|------|------|
| `config.rs` | User `config.json` (default BLE addr) |
| `packet.rs` | `55 55 \| CMD \| LEN \| DATA \| XOR \| AA AA` |
| `protocol.rs` | Commands, B1 PrintStart / page size, models |
| `profile.rs` | `PrinterDevice` aggregate, detected identity, physical capabilities |
| `errors.rs` | Print error 0xDB reason codes |
| `transport.rs`, `transport/` | Common transport/matching + BLE and serial implementations |
| `printer/` | Client core, safe print jobs, queries, validated pacing, explicit raw API |
| `geometry.rs` | Profile-aware mm/pixel conversion, label and safe-area geometry |
| `image_encode.rs` | Image → `Raster` (rows + dimensions together) |
| `font.rs` | System TTF/TTC (`ab_glyph`), named fonts |
| `label.rs` | Square QR + side text; `qr_layout` owns the geometry |
| `main.rs` | Entry point only (parse → dispatch → exit) |
| `cli/args.rs` | clap types + shared arg groups (`ConnArgs`, `TaskArgs`, `FontArgs`) |
| `cli/session.rs` | Connect → print → disconnect; `resolve_task` |
| `cli/commands/` | One module per command group |
| `cli/tips.rs` | Advisory stderr only; never changes behaviour |

### Invariants worth keeping

- **Widths:** physical geometry belongs to `PrinterProfile`; print tasks describe
  wire behavior only. Compose and validate through the connected client's
  profile so model, DPI, and width cannot drift apart.
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
- `0xDB` first byte = `PrinterFault` (0x01 cover, 0x02 no paper, …)
- Info response cmd = `0x40 + key`

Protocol notes: see the command table in `src/protocol.rs`.

---

## Geometry & fonts

- **8 px/mm**; max width **384**
- Printable area on a **charged** B1 is the **whole canvas** — measured with
  `calibrate --boundary`, the last bar (rows 232-239) prints. `SafeArea::B1` is
  1 mm top/bottom as registration margin only. A low battery truncates dense
  pages and looks exactly like a printable-area limit; charge before measuring
- Always use `--label` for full-size media
- Prefer system fonts (`--font-name helvetica|times|arial`) over bitmap fallback
- Default no decorative border; QR is square beside text column
- `--font-size N` for fixed small/large type; omit for auto-fit

---

## Diagnosing a bad print

Read this before changing any layout constant. Most of the effort spent on this
project went into a printable-area limit that did not exist.

### Check the battery first

`thermark info` → `battery: N/4`. At level 1 a dense page sags the supply and
the printer **stops mid-page**. That is indistinguishable from a clipped layout
in a single sample, and it moved the apparent "printable area" by 7 mm between
a flat and a charged battery.

**The tell is inconsistency.** If the same bitmap prints differently twice, it
is power — no buffer-size, pacing, or geometry model produces run-to-run
variation. Chase geometry only after two identical runs agree.

### Then decide what kind of question you have

| Question | How to answer it | Costs a label? |
|---|---|---|
| What exactly will the printer receive? | `thermark print --preview out.png` | no |
| Did output change when it should not have? | `scripts/compare-render.sh <ref>` | no |
| Did a renderer change unexpectedly? | `cargo test --test golden` | no |
| Does content land inside the printable area? | `cargo test --test label_placement` | no |
| Where does this printer actually stop? | `thermark calibrate --boundary` | yes, one |
| Is a deliberate visual change right? | print one label | yes, one |

Only the last two need hardware. Everything above them used to be answered by
printing and photographing, which is slow, ambiguous, and burns media.

### Measure, do not infer

- Photographs of a **curled** label are unreliable: estimating a scale from one
  produced two contradictory measurements of the same printer, out by 5 mm.
  Lay it flat, or read a printed numeral instead of estimating.
- `calibrate --boundary` prints one numbered bar per millimetre, each at its own
  horizontal position. Read the highest complete bar — no counting, no scale
  estimation. If the **last** bar prints, there is no unprintable band at all.
- Quote numbers from the artifact (ink row extents, byte counts), not from
  reading the code.

### Do not let a test encode a theory

A test asserting `safe.bottom > safe.top` locked in a "the feed edge is
unreachable" belief. When the correct value arrived, the test failed and
argued for the wrong number. Pin observable behaviour — ink stays inside the
printable area — not conclusions that have not been verified on hardware.

---

## Protocol behaviour and deliberate omissions

Reference points that are optional, deliberately omitted, already implemented,
or easy to misinterpret. Keep each status explicit. Row-repeat coalescing is
implemented; the encoder splits long runs at the one-byte repeat limit of 255.

1. **`PrinterCheckLine` (0x86)**, payload `[line: u16, 0x01]`, reply `0xd3`.
   Conventionally slotted every 200 rows (`row % 200 == 199`), but it is
   optional and commonly left disabled — it is not required for reliable
   printing, on long pages or otherwise. Closing this "gap" is optional; treat
   earlier notes calling it the clearest gap as superseded.

2. **`PrintBitmapRowIndexed` (0x83)** for sparse rows — used when a row has
   **≤ 6 black pixels**, sending 2-byte pixel indices instead of a bitmap. This
   threshold is a firmware quirk rather than a size optimisation: above 6 black
   pixels the indexed form is reportedly unsafe, and clients refuse to build it.
   Our `Cmd` enum names it but nothing emits it, and no misbehaviour has been
   seen here. The rows that would land under the threshold are hairlines: 1 px
   rules, thin borders, the boundary probe's lettering.

3. **Black-pixel counts can be computed**, not zeroed. The bitmap row packet has
   a three-byte count field, computed against the printhead width in either a
   split (three chunks) or total form; thermark sends three zero bytes. Zeros
   are widely reported to work, and ours do print, so this is likely optional —
   but it is a deliberate deviation, not an accident.

4. **Print direction is per-model.** B-series images top-down; the D11/D110
   family images left-to-right, so clients for those rotate the canvas 90°
   clockwise during encoding and take the column count from the canvas *height*.
   thermark does not rotate; a D110 label is authored narrow-side-first
   (`--label 12x40`) and the wire bytes come out the same. Same output,
   different authoring convention — do not "fix" this by adding a rotation.

5. **`PrintStatus` (0xa3) has a payload worth reading**, and thermark now
   reads it: `[page: i16, pagePrintProgress: u8, pageFeedProgress: u8]`, and in
   the **10-byte form only**, a fault code at offset 6. That fault arrives
   inside a *successful* 0xb3 reply, so the framing layer never sees it — it is
   only catchable by parsing. The progress pair is also the direct answer to
   "how far did it get?", which is the question the battery episode was really
   asking. Do not read offset 6 at other lengths; it is a different field.

6. **`printEnd` returning 0 means refused, not failed.** Polling `printEnd`
   until it returns 1 is a valid completion signal in its own right — thermark
   already retries on `Ok(false)`, which is the same idea.

7. **`labelPositioningCalibration` ejects ~15 cm of paper on B1** when sent 1 or
   2. Deliberately not exposed; there is no way to make that non-destructive to
   a roll.

8. **RFID tells you the consumable, not its size** — see
   [Label size and RFID](README.md#label-size-and-rfid). `consumablesType` could
   auto-select the label type instead of thermark's hardcoded
   `set_label_type(1)`, which is the one place a wrong default costs a mis-feed
   on continuous stock. Not implemented; needs a roll of continuous paper to
   verify.

No label-height limit exists in the protocol — no preset table, no clamp, no
per-model maximum. Page height is bounded only by the `u16` row count. 50x80
media (384×640 px, ~38 KB worst case) is a supported size, not an edge case; it
is simply the one most likely to expose a weak battery.

Also confirmed: row data is written unacknowledged with a fixed inter-packet
interval (**10 ms** is the common figure), which is why thermark paces by bytes
to roughly the same total. Acknowledged writes were tried here and made no
measurable difference.

---

## Pitfalls

1. BLE address match is **exact** by default (full advertising name or id). Short selectors no longer substring-match; use full name from `scan`, or pass `--fuzzy` only if intentional  
2. Require real printer GATT UUID; no random characteristic fallback  
3. Stuck CLI holds BLE lock — mostly addressed: Ctrl-C aborts the local job and
   `BleTransport::drop` blocks until disconnect on a multi-threaded runtime.
   It does not send the protocol's cancel-print command. `SIGKILL` still leaves
   the link held until the printer times out
4. Tiny prints = missing `--label` / wrong canvas, not “broken printer”  
5. Bitmap 5×7 font had mirrored text bugs — do not use for user labels  

---

## Print tasks

`PrintTask` in `src/print_task.rs` selects the on-wire sequence. It does not own
physical geometry.

| Task | Hardware-tested here? |
|------|------------------------|
| `b1` | **Yes** |
| `d11v1`, `d110`, `d110mv4` | No (experimental) |

`PrinterClient` defaults via `PrintTask::for_model`. Override: `--task` / `.with_print_task()`.

CLI: every non-B1 task requires `--allow-experimental` on printing commands.
Library API is unrestricted.

## Tests

```bash
cargo test
cargo test --test golden              # 13 stored renders, pixel-exact
UPDATE_GOLDEN=1 cargo test --test golden   # accept new output, deliberately
scripts/compare-render.sh v0.12.0     # byte-compare renders against a ref
cargo bench --bench image_pipeline    # CPU-only image-pipeline medians
```

Rendering changes are invisible until a label prints, so verify without media:
`--preview` for the exact bitmap, golden tests for unintended changes,
`compare-render.sh` for behaviour-preserving work, `label_placement` for the
printable-band invariant. Print only to confirm a *deliberate* visual change.

- Pure logic: packets, geometry, layout, fonts
- **Mock transport** (`src/mock.rs`): full print job command order, 0xDB errors, summary
- Live BLE print remains manual

Compare benchmark runs on the same host. The benchmark does not measure peak
RSS; use separate processes for memory measurements so allocator state from one
case cannot affect another.

```bash
# coverage (needs Homebrew llvm + cargo-llvm-cov)
export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
cargo llvm-cov --workspace --summary-only
```

## Modernization and simplification

Modernize from evidence, not from file age or line count alone:

1. Check the current stable toolchain and direct crate releases. Keep
   `rust-version`, `rust-toolchain.toml`, CI, and `Cargo.lock` aligned.
2. Prefer compatible lockfile updates (`cargo update`) before considering a
   major dependency bump. Take a major only for a concrete feature, fix, or
   meaningful deletion.
3. Run format, Clippy, the full suite, and the feature-specific library tests
   after dependency or structural changes.
4. Delete duplicate representations and pass typed values through directly.
   A good simplification removes translation code; moving the same code into
   more files is not a simplification.
5. Keep wrappers only when they enforce a policy boundary (safe printing vs.
   raw commands, validated geometry, guaranteed disconnect). Do not collapse
   those boundaries just to reduce line count.
6. Avoid speculative abstractions. Extract code after a rule has at least two
   real callers or when one named owner protects an invariant.

Good candidates for future code reduction:

- Keep tests close to their owner, but count production and test code
  separately before calling a large module a maintenance problem.
- Remove compatibility aliases only in a deliberate breaking release and only
  after confirming they no longer protect real scripts.

Do not simplify away the explicit raw-printer API, transport split, pacing
control flow, effective-width validation, or `label::qr_layout`. Those are
small boundaries around hardware failure modes and measured invariants.

---

## Naming

Product/crate: **thermark**. Interoperability language in docs is fine (“B1-class printers”); avoid vendor branding in crate name.
