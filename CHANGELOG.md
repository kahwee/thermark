# Changelog

All notable changes to **thermark** are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the crate is `0.x`, the public API may change in any minor release; each
such change is listed under **Changed** with the old and new spelling.

## [Unreleased]

## [0.26.0] - 2026-07-31

More of the reference implementation's behaviour, read from
[the protocol reference](the protocol reference) source.

### Added

- **`PrintStatus` (0xa3) replies are now parsed** into
  `printer::info::PrintStatus` — `page`, `page_print_progress`,
  `page_feed_progress`, and the fault code carried in the 10-byte form.
  thermark polled this purely as a keepalive and discarded the body.
- **`MockTransport::set_print_status`** so stalled and faulted pages can be
  modelled without hardware.

### Fixed

- **A fault reported inside a successful status reply now aborts the job.** The
  10-byte form of the 0xa3 response carries an error code at offset 6. It comes
  back in a normal `0xb3` packet, not a `0xDB` error packet, so the framing
  layer never saw it and the job pressed on to 50 `PrintEnd` retries and a
  useless `PrintNotConfirmed`. Only the 10-byte form is read; at other lengths
  that offset is a different field.

### Changed

- **Status polling stops as soon as the page reports fully imaged and fed**
  instead of always running the full budget (8 polls on B1). Saves up to ~1.2 s
  per print.
- **A page that stops short is now logged with its progress** —
  `printer stopped short of a complete page — check the battery`, with the
  actual percentages. This is the number that would have identified the low
  battery immediately instead of after a session of layout changes. It is a
  warning, not an error: `PrintEnd` remains the authority on completion.

- **`thermark info` now names the loaded consumable** — a `paper:` line with the
  type (gapped, black-mark, continuous, transparent, …) decoded from the RFID
  tag's `consumables_type`, plus labels remaining when the tag reports counts.
  Previously this was printed as a bare integer. `RfidInfo::consumable_type_name`
  and `RfidInfo::labels_remaining` are public; the latter returns `None` rather
  than 0 for the printer's `-1` "not reported" sentinel.

### Documentation

- **README: "Does the printer know what size paper is loaded?"** No — not in
  millimetres. The RFID tag carries barcode, serial, label counts and consumable
  *type*, but no geometry; vendor software resolves size from the barcode
  through its own catalogue. `--label` is not optional and cannot be inferred.
  `thermark info` now says so directly instead of hinting that the barcode
  encodes the size.
- **`AGENTS.md`**: four more reference-implementation findings — the status
  payload, `printEnd` returning 0 as *refused*, `labelPositioningCalibration`
  ejecting ~15 cm of paper on B1 (deliberately not exposed), and RFID
  `consumablesType` as a possible source for the label type thermark currently
  hardcodes.

## [0.25.0] - 2026-07-31

Non-50x30 media. Verified offline across 40x20, 40x30, 50x30 and 50x80; every
size lays out inside its printable area and no golden image changed.

### Fixed

- **The boundary probe now adapts to the loaded media.** `BOUNDARY_FROM_MM` /
  `BOUNDARY_TO_MM` were fixed at 17..29 — correct only for 50x30. On 40x20 that
  drew three bars, and on 50x80 it drew a staircase across the middle of the
  label, measuring nothing. Replaced by `label::boundary_range(label)`, which
  marks the last `BOUNDARY_SPAN_MM` (13) millimetres, ending at the final
  drawable one. On 50x30 it still yields 17..29, so goldens are unchanged.

### Changed

- **`label::BOUNDARY_FROM_MM` and `label::BOUNDARY_TO_MM` are gone**, replaced by
  `label::BOUNDARY_SPAN_MM` and `label::boundary_range(LabelPx)`.

### Added

- **`tests/label_placement.rs`: two multi-size tests.** One renders text and QR
  labels on all four common roll sizes and asserts the ink stays inside the safe
  area; the other asserts the probe's last bar is the media's last millimetre.
  These caught the hardcoded range.
- **README: a media-size table and an "I switched to a different roll" FAQ** —
  pixel dimensions, largest QR, and worst-case page bytes per size, plus the two
  real constraints (width clamps to 384 px; narrow tape has no room for a QR
  beside text).
- **Protocol notes corrected against the protocol reference.** `PrinterCheckLine` (0x86) is
  opt-in and off by default there, so it is not required for reliability —
  earlier notes calling it the clearest gap are superseded.
  `PrintBitmapRowIndexed` (0x83) is documented as a firmware quirk ("printer
  powers off if black pixel count > 6"), not a size optimisation. Added the
  per-model print direction (D11/D110 encode rotated) and a warning that
  `repeat` is one byte, so future row coalescing must cap runs at 255.

## [0.24.0] - 2026-07-31

Documentation only — `compare-render.sh` reports every output identical.

### Added

- **`AGENTS.md` → "Diagnosing a bad print".** Check the battery before touching
  a layout constant; the tell for a power problem is *inconsistency between
  runs*, since no buffer or pacing model produces run-to-run variation. Plus a
  table of which question each tool answers and which of them cost a label
  (only two do), and why a curled-label photograph is not a measurement.
- **`AGENTS.md` → "Learned from the reference implementation".** Four things
  [the protocol reference](the protocol reference) does that thermark
  does not:
  - coalesces consecutive identical rows via the `repeats` field
  - sends `PrinterCheckLine` (0x86) every 200 rows
  - uses `PrintBitmapRowIndexed` (0x83) for rows with ≤ 6 black pixels
  - computes the black-pixel counts thermark sends as zeros
- `Cmd::PrinterCheckLine` (0x86) added to the protocol enum, documented as
  unimplemented, so a known gap is visible in code rather than only in prose.
- `print_bitmap_row` documents why `black_counts` is zeroed and `repeats` is
  always 1 — both deliberate deviations, neither an oversight.

## [0.23.0] - 2026-07-31

**The printable area was never small.** Measured on a fully charged printer,
`SafeArea::B1` drops from a 40 px bottom inset to 8 px, returning **4 mm of
every label**.

### Measurement

`thermark calibrate --boundary` at battery 4/4: the **29 mm** bar — the last
one the probe can draw, covering rows 232-239 — printed complete and reached
the label's edge. The printer addresses the whole 240-row canvas.

Every earlier reading was taken at battery 1/4, where a dense page sags the
supply and the printer stops mid-page. That is indistinguishable from a
printable-area limit in a single sample. Charging moved the apparent "limit" by
7 mm, which is how we know it was never a limit.

| | printable area | QR side |
|---|---|---|
| before | 48.0 x 24.0 mm | 184 px |
| after | **48.0 x 28.0 mm** | **216 px** |

The 1 mm retained top and bottom is registration insurance, not unreachable
area — labels do not feed identically every time, and a circle placed exactly
on row 0 came back with its top shaved. `SafeArea::NONE` gives true full bleed.

### Changed

- `SafeArea::B1` is `top: 8, bottom: 8, left: 0, right: 0`.
- `SafeArea`'s documentation no longer claims the feed edge is asymmetrically
  unreachable, and warns against inferring the value from one clipped print.
- `calibrate --boundary` says what it means when the *last* bar prints — that
  there is no unprintable band at all — and to charge before measuring.
- Goldens regenerated: layouts legitimately change with the larger area.

### Fixed

- A test asserted `safe.bottom > safe.top`, encoding the incorrect
  "feed edge is unreachable" theory. It defended a wrong belief and would have
  resisted this correction; it now asserts only that insets are applied
  correctly.

## [0.22.0] - 2026-07-31

Modern-std pass. Behaviour-preserving — `compare-render.sh` reports every
output identical.

### Changed

- `LabelMm::to_pixels` uses `checked_next_multiple_of(8)` instead of
  `div_ceil(8) * 8`. It states the intent — round the width up to a byte
  boundary — and returns `None` on overflow where the multiply could wrap. That
  wrap is the original `--label infx30` bug, so the guard is now in the
  arithmetic rather than only in the validation ahead of it.

### Added

- `image_encode::ink_bounds` — bounding box of drawn pixels, or `None` if
  blank. "Where did anything actually get drawn?" was being answered by
  hand-rolled loops in `trim_white` and in two tests; separate implementations
  of that question are how call sites end up disagreeing about what counts as
  ink. `trim_white` and `tests/label_placement.rs` now share it.

### Notes on what was considered and rejected

`u32::midpoint` and `f32::midpoint` do not fit the centring code: that computes
half of a *difference* (`(space - content) / 2`), not the midpoint of two
values, and the subtraction cannot overflow.

`slice::chunk_by` would fit the row encoder well — consecutive identical rows
could collapse into a single packet using the `repeats` field, which is already
in the wire format and always set to 1. That is the largest remaining byte
reduction and would matter for dense pages. It changes what the printer
receives, so it needs verification on hardware rather than a mock.

## [0.21.0] - 2026-07-31

### Fixed

- **Lint-clean under every feature combination.** `--no-default-features` left
  two unused imports in `doctor.rs`, since only a transport actually talks to a
  printer. Gated on the features that use them rather than silenced with
  `#[allow(unused)]`, so a genuinely unused import is still reported. All four
  combinations — default, none, `ble` only, `serial` only — are now clean, as
  is `clippy --all-targets -D warnings`, the form CI runs.

### Removed

Six public functions with zero callers anywhere in the crate:

- `PrinterClient::with_simple_print_start` — superseded by `with_print_task`
- `PrinterClient::transport_mut`
- `protocol::{set_quantity, default_response_cmd, page_start, page_end}`

`protocol::cancel_print` is kept despite having no caller: it is needed to stop
a job on Ctrl-C, which is a known gap.

### Changed

- Dependencies updated (`image` 0.25.10, `moxcms` 0.8.1, `tokio-macros` 2.7.2).
- `Model`'s two `impl` blocks merged into one.

No rendering change — `compare-render.sh` reports every output identical.

## [0.20.0] - 2026-07-31

### Changed

- **`rust-version` is now `1.97`**, the current stable, so new language
  features can be used without checking an older floor first. This is a
  minimum, not a request for a toolchain: anyone below it cannot build, which
  is a deliberate trade for a personal CLI rather than a library others pin.

### Added

- `rust-toolchain.toml` pinning the `stable` channel with rustfmt and clippy.
  Honoured by rustup; a Homebrew or distro `cargo` ignores it and uses what it
  ships, which is fine as long as that meets `rust-version` — cargo says so
  clearly if not.
- CI installs stable explicitly (`dtolnay/rust-toolchain@stable`) and prints
  the version. The runner image ships some Rust, but it can lag behind
  `rust-version`, and then the build fails for a reason unrelated to the change
  under test.

## [0.19.0] - 2026-07-31

Correctness and edition hygiene. No rendering change — `compare-render.sh`
reports every output identical to v0.18.0.

### Fixed

- **Config writes are atomic.** `fs::write` truncates in place, so an
  interruption mid-write left a half-written `config.json` that every later
  command failed to parse. Now writes a sibling temp file and renames over the
  target; a reader sees either the old file or the new one, never a fragment.
- **`rust-version` was wrong.** The crate uses a let-chain, stabilised in 1.88,
  while declaring 1.87 — so an older toolchain failed with a parse error rather
  than cargo's clear "requires rustc 1.88".

### Changed

- **No `unsafe` left in the crate.** Five test sites called
  `std::env::set_var`, which is `unsafe` in edition 2024 because a concurrent
  read from another thread is undefined behaviour — and `resolve_addr` reads
  the environment, in a suite that runs in parallel. Environment values are now
  parameters (`resolve_addr_with`, `default_path_with`), read once at the CLI
  edge, so the tests need no global mutation and the `ENV_LOCK` mutex they
  shared is gone.
- Collapsed nested `if let`s into edition-2024 let-chains, and replaced a
  `map(..).unwrap_or(false)` with `is_some_and`.

## [0.18.0] - 2026-07-31

### Fixed

- **A QR too dense to scan is now an error, not a sticker.** Long content means
  more modules and fewer pixels each; below 2 px per module heat bleed closes
  the gaps and the code is unreadable, but it still *looked* like a QR. A
  900-byte payload on a 200 px square rendered 1 px modules. Warns below 3 px,
  refuses below 2.
- **`escape_wifi_field` now escapes `:`**, which is reserved in the WIFI QR
  format. Lenient phone parsers coped, but a strict reader mis-parses a
  password containing a colon.
- **SSID length is checked in bytes, not characters.** The 802.11 limit is 32
  *bytes*: a 12-character Japanese SSID is 36 bytes and invalid, and was being
  accepted.

### Added

- An **FAQ** in the README covering what this project actually got wrong in
  practice: distinguishing a low battery from a printable-area limit (the tell
  is inconsistency between runs), measuring the real area with
  `calibrate --boundary`, checking output with `--preview` instead of a label,
  why artwork can print smaller than the media, and why a QR may not scan.

## [0.17.0] - 2026-07-31

### Added

- **`thermark config set --label 50x30`** — a default label size, so `--label`
  need not be repeated on every command. Resolution is CLI flag, then saved
  config, then `50x30`, matching how `model`, `connection`, and `scan_secs`
  already work. The size is validated when saved, rather than failing later on
  every command with no hint where it came from.

### Changed

- `--label` is optional everywhere instead of carrying a hardcoded `50x30`
  default in five separate command definitions.

## [0.16.0] - 2026-07-31

### Changed

- **Battery is reported as a level out of 4, with what it means.** `info`
  printed a bare `battery: 1`, which reads like a unit rather than a warning —
  and a low battery makes dense pages print only partway, which is easy to
  mistake for a layout bug. Now:

  ```
  battery:      1/4  (low — dense or dark labels may print only partway; charge it)
  ```

  `doctor` uses the same wording via `printer::describe_battery`, so the two
  cannot describe the same battery differently.

### Note

The protocol reports a 0–4 level and no charging flag, so thermark can tell you
the level but not whether the printer is currently charging — check the
device's own indicator for that, then re-run `thermark info` to confirm the
level is climbing.

## [0.15.0] - 2026-07-31

Dense pages were printing only partway. It was not the printable area, and not
the streaming — it was the battery.

### Root cause

`thermark info` reports **battery level 1 of 4**, which `doctor` already
classifies as "low — charge before long jobs". A dense page fires far more
heating elements than a sparse one and draws much more current; on a low
battery the supply sags and the printer stops mid-page, which looks exactly
like a clipped label.

Everything observed fits, including the part that ruled out the alternatives:

| Page | bytes | reached |
|---|---|---|
| boundary probe | 8.2 KB | row ~217 |
| QR | 11.2 KB | complete |
| calibration | 14.6 KB | row ~176 |

…and **repeat runs of the same page differed**. No buffer-size or pacing model
produces run-to-run variation; a battery recovering between prints does.

### Added

- **Low-battery warning before printing.** Only an empty battery blocked a job;
  a low one said nothing, so the failure appeared as mysterious clipping. Now
  warns and suggests charging or reducing `--density`.
- `THERMARK_SLOW=1` selects `Pacing::CAREFUL`, kept as a diagnostic for
  distinguishing "sent too fast" from other causes.

### Changed

- Row streaming paces by **bytes** rather than rows, comparable to the
  reference implementation's fixed 10 ms per packet. A dense page now gets
  ~2.3 s of pacing instead of the ~1.4 s it received regardless of size.

### Reverted

- Acknowledged BLE writes (`WriteType::WithResponse`). Tried against real
  hardware, made no difference, and deviates from the protocol reference, which ships
  `writeValueWithoutResponse`. Not worth keeping on a hunch.

### Note on `SafeArea`

Left unchanged. The measured "printable area" numbers from this investigation
are contaminated by the battery, so tightening the inset against them would
bake in an artefact. Re-measure with `thermark calibrate --boundary` on a
charged printer.

## [0.14.0] - 2026-07-30

### Added

- **`thermark calibrate --boundary`** — a staircase of numbered bars, one per
  millimetre from 17 to 29, each at its own horizontal position. The highest
  number whose bar printed is where the printer stops. No counting, no
  estimating a scale from a photograph.

  The existing feed ruler puts ticks 1 mm (8 px) apart, too crowded to letter
  every one, so readings off it were "somewhere past 20" plus arithmetic on a
  photo — and that produced two contradictory measurements of the same printer:
  a full-bleed calibration whose ink stopped at ~22.5 mm, and a QR label that
  printed cleanly with ink to 23.6 mm. Separating each millimetre horizontally
  removes the estimation entirely.

### Notes

Those two measurements disagreeing is itself the finding: **registration varies
between labels**, so a single measured cutoff is not a safe inset — the safe
inset has to cover the worst case across several. `SafeArea::B1` is unchanged
pending readings from the probe; the legend now says to use the lower number if
two runs differ.

## [0.13.0] - 2026-07-30

Adds the verification that this project was missing. Nearly every bug this
month was a rendering change nobody could see until a label came out of the
printer; both harnesses here make those visible on `cargo test`.

### Added

- **`tests/golden.rs`** — golden-image tests for 13 renders: QR (both text
  sides), text (centred, left, small fixed size, long-wrapping), Wi-Fi,
  calibration (plain, full-bleed, numbered), and artwork placement (contain,
  fill, trimmed, full-bleed). Each is compared pixel by pixel against a stored
  reference.

  On a mismatch it reports the differing pixel count and first coordinate, and
  writes the actual render to `target/golden-actual/` so the change can be
  looked at rather than guessed at. `UPDATE_GOLDEN=1 cargo test --test golden`
  accepts new output deliberately.

  Verified by perturbation: changing the layout margin by 2 px failed exactly
  the five font-dependent cases, with counts and coordinates, and left the
  geometry cases alone — which is correct, since artwork placement uses the
  safe area rather than that margin.

  Text cases skip where the expected system font is absent; a companion test
  asserts the geometry cases never skip, so a bare checkout still has cover.

- **`scripts/compare-render.sh <ref>`** — builds any git ref in a throwaway
  worktree and byte-compares CLI renders against the working tree. Answers
  "did this change alter any output?" for work that is meant to be
  behaviour-preserving. Covers the CLI end to end, including flag handling, and
  works against refs that predate the golden suite. Confirms v0.12.0 changed
  nothing versus v0.11.0.

### Notes

Which tool for which question:

| Question | Tool |
|---|---|
| Did output change when it should not have? | `scripts/compare-render.sh <ref>` |
| Did a renderer change unexpectedly? | `cargo test --test golden` |
| Does content land inside the printable band? | `cargo test --test label_placement` |
| What exactly will the printer receive? | `thermark print --preview out.png` |
| Is a deliberate visual change actually right? | print one label |

## [0.12.0] - 2026-07-30

Consolidation of the layout code, after a run of bugs that all traced to the
same shape: two places computing the same geometry, and one of them drifting.
No behaviour change intended — `thermark text` renders byte-identically before
and after.

### Changed

- **One owner for the content box.** `label::content_box(label, safe)` returns
  the printable area inset by the cosmetic margin. `qr_layout` and
  `make_text_label` both call it; previously each computed it, and the text
  path silently drifted onto the raw label.
- **`max_qr_side` takes a `SafeArea`.** It used the default while the caller
  rendered with a configured one, so `thermark qr` could log a QR size that did
  not match the label it printed.
- **`qr_layout` takes a `SafeArea`** (the `qr_layout_in` split is gone).
- **One name per placement operation.** `fill_label` and `contain_label` now
  take a `SafeArea` directly; the `_in` variants and the wrappers
  `fill_label_with_margin`, `pad_to_label` are removed. Two names for the same
  operation is how the `contain_label` centring fix got applied to the wrapper
  nobody called.
- **`calibration_pattern` takes the `SafeArea`** rather than having a second
  `_with` entry point.

### Added

- `image_encode::CALIBRATION_RULER_MAJOR_PX` / `_MINOR_PX`. The calibration
  numerals were positioned at a hardcoded x=30 chosen to clear ticks of length
  26 — two constants that had to agree by hand. The numerals now derive their
  inset from the tick length, and skip lettering entirely on labels too narrow
  to fit them without collision.

### Removed

- `image_encode::pad_to_label`, `fill_label_with_margin`, `fill_label_in`,
  `contain_label_in`, `calibration_pattern_with`, `label::qr_layout_in` — dead
  or duplicate entry points.

## [0.11.0] - 2026-07-30

### Fixed

- **`make_text_label` never used the safe area.** It laid text out on the raw
  label with only the cosmetic margin, giving a 232-row box on 50x30 media
  instead of 184 — so text was sized for space the printer cannot reach. An
  earlier patch meant to fix this silently failed to apply, and the placement
  test passed anyway because metric centring happened to leave the ink one row
  inside the band. Optical centring shifted it two rows out and exposed the
  real bug underneath.
- **Text was centred on font metrics rather than on its ink.** `ascent`
  reserves room above cap height that most glyphs never occupy, so a centred
  block sat low: 27 rows of slack above and 1 below. Placement now measures the
  rendered glyph outlines (`LabelFont::block_ink_bounds`) and centres those —
  16 above, 17 below on the same label.

### Added

- `LabelFont::block_ink_bounds` — vertical ink extent of a wrapped block
  relative to its first baseline, from glyph outlines rather than metrics.
- A test asserting text is optically centred, not merely inside the band. The
  band check alone could not tell "correct" from "one row lucky".

## [0.10.0] - 2026-07-30

Audited every path that places content on a label, instead of fixing them one
print at a time. The audit found two more bugs immediately.

### Fixed

- **Text labels overflowed the printable area.** `LabelFont::text_height`
  returns the whole line box (ascent + descent + line gap), but
  `draw_text_block` used it as the offset to the *first baseline*, which should
  only be the ascent. Every line sat a descender too low and the last one ran
  past the bottom of its box — measured at row 212 of a band ending at 200, so
  the final line of text was being clipped on real prints. Adds
  `LabelFont::ascent`.
- **Artwork clipped at the very top.** `SafeArea::B1.top` was 0 on the evidence
  that rings at inset 0 print. They do, but only just: once trimming made
  artwork's topmost ink land exactly on row 0, a printed circle came back with
  its top shaved flat. Registration varies by a fraction of a millimetre
  label-to-label, so ink does not belong on the extreme edge even where that
  edge nominally prints. Now 8 px (1 mm).

### Added

- `tests/label_placement.rs` — asserts that **every** label path (qr, text,
  wifi, and trimmed raw artwork in both fit modes) keeps its ink inside the
  printable band. Both bugs above were caught by this on first run, without
  hardware. Previously each placement path was verified only by printing it,
  which is how they escaped one at a time.

## [0.9.0] - 2026-07-30

Artwork now fills the label instead of floating in it.

### Fixed

- **Source images kept their own white border, which was added to the label's
  inset.** A drawing carrying a 35 px margin lost another ~29 rows to it after
  scaling, on top of the 40 rows reserved at the feed edge — so the bulldozer
  occupied 167 of 200 usable rows and looked like it had been cut short, even
  though nothing was clipped. `thermark print` now trims uniform background
  from the source before placing it. The same artwork now spans rows 0–199 of
  the 200-row band.

  Measured on the exact bitmaps sent to the printer, not inferred from a photo:

  | | ink rows | white below |
  |---|---|---|
  | before | 4–170 | 69 |
  | after  | 0–199 | 1  |

### Added

- `image_encode::trim_white`, and `thermark print --no-trim` to keep the
  border when it is deliberate.

### Notes

Trimming is off for rendered stickers and the calibration pattern: their canvas
*is* the layout, and cropping would rescale and undo the placement. The
calibration pattern in particular must stay full-bleed and untrimmed, since it
is the instrument that measures the edges.

## [0.8.0] - 2026-07-30

Calibration validated against hardware; the printable area is now confirmed
rather than estimated.

### Measurement

Flat photo of the numbered calibration, using the printed numerals as scale:

- The **safe-area box printed complete on all four sides**, so the 5 mm bottom
  inset is correct. Blank label below it measures ~5.4 mm, which *is* that
  inset — the printer itself stops around row 203 of 240, leaving ~4.6 mm
  unreachable. Setting and hardware agree to within half a millimetre.
- Horizontally the print is **not centred on the label**: ~2.4 mm of white on
  the left, flush on the right. The printhead is 48 mm on a 50 mm label, so
  2 mm is always unprintable — but all of it is currently on one side, which
  means the roll is sitting off-centre in the paper guide. No software setting
  moves it; the head window is fixed and the canvas already spans all 384 dots.

### Added

- `thermark calibrate` prints millimetre **numerals** down both rulers, so a
  photo is self-describing — read the last number rather than counting ticks
  from an edge that may itself be clipped.
- It also reports the **usable print area in mm**, which is what artwork should
  be designed against rather than the label's nominal size.

### Fixed

- Numerals sat *below* their tick, so the most important reading — the last one
  before the printable band ends — was the one guaranteed to be cut off. They
  now sit above the tick.

## [0.7.0] - 2026-07-30

### Fixed

- **`thermark print` ignored the printable area entirely.** Only `qr`, `text`,
  and `wifi` respected `SafeArea`; raw images were scaled across the whole
  240-row canvas, so anything in the bottom ~40 rows went straight into the
  band the printer never reaches and was silently lost. This is why a sticker
  could still come out clipped after the safe area was measured and saved —
  the setting simply was not consulted on that path.
- **`contain_label` centred on the raw canvas rather than the printable box**,
  which pushed content toward the feed edge even once the area was known.

Neither is a protocol problem, which is where I had been looking. A regression
test now asserts that no ink lands outside the printable area for either
placement mode, and that `SafeArea::NONE` still reaches the true edges (the
calibration pattern depends on that).

### Added

- **`thermark print --preview <png>`** — writes exactly what would be sent and
  prints nothing. Composition runs through the same `compose_for_label` the
  real print path uses, so placement can be checked without a printer or a
  wasted label.
- **`thermark print --full-bleed`** — opt out of the inset and use the whole
  canvas, for media whose feed edge really is printable.
- `printer::compose_for_label`, `image_encode::fill_label_in` /
  `contain_label_in` taking an explicit `SafeArea`.

### Changed

- Rendered stickers and the calibration pattern now pass `SafeArea::NONE` when
  they hand their PNG to the print path: they have already laid out inside the
  printable area, and insetting a second time would shrink them twice. The
  calibration pattern in particular must stay full-bleed — it is the
  instrument that measures those edges.

## [0.6.0] - 2026-07-30

Measured the B1's real printable window instead of guessing at it.

### Measurement

`thermark calibrate` with the feed ruler, photographed flat, on B1 + 50x30:

| Edge   | Observed                                       |
|--------|------------------------------------------------|
| top    | ~2.4 mm of label before the first printed row  |
| band   | ~25.3 mm printed                               |
| bottom | ~2.3 mm of label after the last printed row    |

Ruler ticks are **evenly spaced**, so the image is not scaled or compressed —
rows past the window are simply dropped. The printer begins its window a little
after the label's leading edge, which is why canvas row 0 already lands inside
the printed band while the last ~40 rows never reach paper.

Two of these are hardware limits that no setting can fill: the ~2 mm at the
feed edges, and the 2 mm across (a 48 mm printhead on a 50 mm label). A
lopsided left/right border means the roll is off-centre in the paper guide.

### Changed

- `SafeArea::B1` is now `top: 0, bottom: 40, left: 0, right: 0`. The previous
  value inset all four edges, which shrank artwork on three edges that print
  fine — the loss is at the feed edge only, and the top needs no inset at all
  because the printer's own offset already places row 0 inside the window.
- The `calibrate` legend now says which white borders are physical, so this
  does not get chased as a bug again.

### Added

- `thermark config safe-area --last-tick <mm> --label 50x30` — report the last
  ruler tick that printed and it computes the inset for you, rather than
  making you do the arithmetic.

## [0.5.0] - 2026-07-30

Makes the printable-area problem measurable and configurable instead of
hard-coded, after a 5 mm bottom inset still clipped on real hardware.

### Added

- **`thermark config safe-area --top/--bottom/--left/--right <mm>`** — save the
  insets measured with `thermark calibrate`. Values are millimetres in, pixels
  stored; omitted edges keep their current value; `--reset` returns to the
  built-in default. Shown by `thermark config show` and used by the `qr`,
  `text`, and `wifi` label paths, so a measurement no longer needs a rebuild.
- **A feed ruler on the calibration pattern** — a tick every 1 mm down both
  edges, long every 5 mm. The rings only resolve 0.5 mm near the very edge;
  the ruler tells you exactly which row the print stops at, which is the
  number needed to size the bottom inset.
- `SafeArea::from_mm`, and serde support so it round-trips through
  `config.json`.

### Changed

- `QrLabelOptions`, `TextLabelOptions`, and `WifiLabelOptions` take a `safe:
  SafeArea` field rather than reading a global default, so the CLI can pass the
  configured value.
- `SafeArea::B1`'s bottom inset is 40 px (5 mm), up from 20 px, per the
  calibration ruler. **This is provisional** — see the note below.

### Fixed

- The calibration legend printed the *default* safe area while the pattern drew
  the *configured* one: a `let safe = SafeArea::default()` shadowed the
  function parameter, so the legend could contradict the label in your hand.

### Known issue

The bottom edge can still clip. Research points at a registration problem
rather than an unprintable margin: community reports reports the printer
starting to print before the label's leading edge, so the leading rows land on
the gap between labels and the trailing rows fall off the end. If that is what
is happening here, shrinking the content (what `SafeArea` does) is the wrong
remedy — the content needs to be *offset*, not made smaller. The feed ruler
added here is what distinguishes the two.

## [0.4.0] - 2026-07-30

Driven by printing on real B1 hardware and reading the results.

### Added

- **`thermark text`** — a text-only sticker. Previously the only way to print
  words was `qr`, which always draws a QR beside them, so "just print this
  text" was not expressible. Supports `--align left|center|right` and the same
  font, label, and density flags as the other sticker commands.
- **`geometry::SafeArea`** — per-edge printable insets, and deliberately *not*
  symmetric. Calibration on B1 with 50×30 media shows rings at inset 0 printing
  cleanly along the top and both sides, while the last ~2 mm at the feed
  (bottom) edge is lost: the label clears the printhead before the final rows
  are laid down. Padding all four edges equally would give away good label area
  on three sides to fix a problem on one. QR, text, and Wi-Fi labels now lay
  out inside this area.
- **`examples/bulldozer.rs`** — a line-art sticker built from signed-distance
  primitives. Outlines rather than fills: large solid areas bleed on thermal
  paper, drain the battery, and read as a blob.

### Changed

- **`thermark calibrate` is now a measuring instrument.** It draws six
  concentric rings 0.5 mm apart plus a thick box showing the *configured* safe
  area, and prints a legend explaining how to read them. One print answers "is
  my safe area actually inside the printable region?" — the old single border
  could only say that something clipped, never how much.
- `image_encode::calibration_pattern_with` takes an optional `SafeArea` to
  outline; `calibration_pattern` uses the default.
- `QrLayout` exposes the `Rect` it was fitted into instead of a bare `margin`,
  and centres the QR within the printable band rather than the raw canvas.
- `label::draw_text_block`, `TextAlign`, and `geometry::Rect` are public, shared
  by the QR, text, and Wi-Fi label paths.

### Fixed

- **Auto-fitted text split words mid-way.** `fit_size` chose purely on whether
  the wrapped lines fit — but a hard-broken line *does* fit, so `THERMARK`
  rendered as `THER` / `MARK` at a large size instead of staying whole at a
  smaller one. It now prefers the largest size at which every word fits
  intact, falling back to splitting only when even `MIN_FONT_PX` cannot hold
  the longest word. Confirmed on a printed label.
- **A full-height QR clipped at the bottom edge**, losing roughly 1% of the
  code. Layouts now respect the measured `SafeArea`.
- `Model`, `PrintTask`, `SupportStatus`, and `ConnPref` `Display` impls use
  `f.pad`, so `{:<10}` actually aligns them.

## [0.3.0] - 2026-07-30

Reliability of the BLE link. No API breaks, but the on-wire behaviour changes:
the printer now sees repeat reads when a request goes unanswered, so this is a
minor rather than a patch release.

### Fixed

- **An interrupted or failed job could leave the printer connected.**
  `BleTransport`'s `Drop` spawned a *detached* task to disconnect, which is not
  guaranteed to run — and the common case is `main` returning right after an
  error, shutting the runtime down before that task is ever polled. The printer
  then stays connected, holding the single-client BLE lock until it times out.
  On a multi-threaded runtime `Drop` now blocks until the disconnect completes;
  on a single-threaded one (`#[tokio::test]`) it stays best-effort, since
  blocking there would deadlock.
- **Ctrl-C left the link held.** `SIGINT` now cancels the in-flight command and
  drops the open session — which, with the fix above, disconnects the printer
  before exiting `130`. Previously the process exited from under the session.
- **A lost BLE write was unrecoverable.** `transceive`'s `attempts` controlled
  how many times it *waited*, but the request was sent exactly once — and BLE
  writes go out unacknowledged (`WriteType::WithoutResponse`), so a dropped
  request is indistinguishable from a slow printer and no amount of waiting
  recovers it. Reads and idempotent settings now resend on each attempt.

### Added

- `printer::OnTimeout` and `PrinterClient::transceive_with`, selecting resend
  behaviour per command. `OnTimeout::Resend` is used only where acting twice
  equals acting once — `PrinterInfo`, `Heartbeat`, `RfidInfo`, `PrintStatus`,
  `SetDensity`, `SetLabelType`, `PrintClear`. The state-advancing steps
  (`PrintStart`, `PageStart`, `SetPageSize`, `PageEnd`, `PrintEnd`) remain
  single-send: if the *reply* was what got lost, the printer already acted, and
  a second `PrintStart` would start a second job.
- `MockTransport::drop_first_writes(cmd, n)` to simulate lost writes in tests.
- `tokio`'s `signal` feature, for the Ctrl-C handler.

### Unchanged

`PrinterClient::transceive` keeps its signature and its single-send semantics
(`OnTimeout::WaitOnly`), so existing callers are unaffected.

## [0.2.0] - 2026-07-30

First release with a stable module layout. Two of the fixes below are crashes
reachable from ordinary command lines, so upgrading is recommended.

### Fixed

- **`--label` accepted `inf` and `nan`.** `--label infx30` overflowed the
  multiply in `LabelMm::to_pixels`; because release builds disable
  overflow checks it wrapped and silently printed an 8px-wide label instead of
  panicking. `--label nanx30` bypassed the `<= 0.0` guard entirely, since NaN
  compares false against everything. Label dimensions must now be finite,
  positive, and at most 1000 mm, and `mm_to_px` clamps independently so the
  pixel math is total no matter how it is reached.
- **Every QR on a D11/D110 panicked.** The text-column width used
  `clamp(64, width / 2)`, which panics whenever `min > max` — true for any
  label under 128px wide, i.e. the entire 96px narrow-head family. Labels too
  narrow to hold a usable QR now return a clear error instead.
- **Oversized packets were silently corrupted.** `Packet::encode` cast the
  payload length to `u8`, so a 300-byte payload went out with `LEN=44` and a
  checksum computed over the wrong length — an undecodable frame, reported as
  success. Encoding is now fallible.
- **Mid-job printer faults were swallowed.** A printer reporting "out of paper"
  via `0xDB` during the status poll was discarded by a `let _ =`, followed by 50
  futile `PrintEnd` retries and a `PrintNotConfirmed` that named nothing. The
  printer's own reason now propagates.
- **`fit_size` could return a size larger than the smallest it tried**, so text
  that already overflowed at 10px was rendered at 12px and overflowed further.
- `thermark tasks` columns are aligned again — the `Display` impls used
  `write!`, which ignores the formatter's width.

### Changed

Library API. The CLI is unaffected except where noted.

- `Packet::encode` returns `Result<Vec<u8>, PacketError>`; added
  `Packet::try_new` for caller-supplied data and `Packet::MAX_DATA_LEN`.
- `Packet::checksum` takes the length explicitly, so it can no longer be
  computed over a length that disagrees with the wire.
- `image_encode::encode_image{,_opts,_path,_path_opts}` collapse into
  `image_encode::encode` and `encode_path`, both returning a `Raster` that
  carries its rows and dimensions together. Rotation moved out to
  `image_encode::rotate`, which takes a `Rotation` — the `rotate_deg: u32`
  parameter is gone, along with `Error::InvalidRotation`'s only source.
- `PrinterClient::print_rows(width, height, rows, density)` becomes
  `print_raster(raster, density)`.
- `PrinterClient::with_pace(bool)` becomes `with_pacing(Pacing)`. The old flag
  changed the retry counts as well as the sleeps, so tests exercised a
  different control flow than production; `Pacing::REAL` and `Pacing::INSTANT`
  now differ only in duration.
- `BleDeviceInfo`'s fields move behind a `BleCandidate`, whose `name` is an
  `Option<String>` rather than a `"(no name)"` sentinel compared as data.
  `score_ble_candidate` and `select_ble_candidate` take `&BleCandidate`, and
  `select_ble_candidate` returns the crate's `Error` instead of a bare `String`.
- `doctor::run_doctor` takes a `DoctorOptions` struct instead of five
  positional arguments, and reports the task a print would actually use —
  `evaluate_print_task` now accepts an explicit `Option<PrintTask>`.
- Removed the unused 5×7 bitmap font API (`draw_text_bitmap`, `glyph`,
  `wrap_text`, `chars_fit`, `GLYPH_*`, and the free `draw_text` that shadowed
  `LabelFont::draw_text`). It had no callers, covered ~15 characters, and
  `AGENTS.md` already warned against using it for labels.
- Removed `image_encode::test_pattern`, which was unused and panicked on
  zero-sized input.

### Added

- `print_task::effective_max_width_px(model, task)` — the width limit that
  actually applies to a job. Previously the canvas was sized from the model
  alone while the raster was checked against both, so a mismatched
  `--model`/`--task` encoded the entire image before being rejected.
- `geometry::HEAD_WIDE_PX` / `HEAD_NARROW_PX` — printhead widths in one place
  instead of two parallel tables in `Model` and `PrintTask`.
- `label::qr_layout` — one source of truth for the QR-beside-text geometry.
  `max_qr_side` was a verbatim copy of the same math, carrying the same panic.
- `doctor::Check::{pass, warn, fail}` constructors, replacing ~48 hand-built
  struct literals.
- `thermark doctor --task` to report on a specific print task.

### Internal

- The 1094-line `main.rs` is now ~35 lines; the CLI lives in `src/cli/` split
  into `args`, `session`, `tips`, and one module per command group. The
  connect/print/disconnect sequence that appeared in four commands is now
  `session::print_file`, and the repeated `--task` / font / connection flags
  are shared clap argument groups.
- Test count 144 → 157, with regression coverage for each fix above.

## [0.1.0] - 2026-07-28

Initial release: BLE and USB serial transports, B1 print task, QR and guest
Wi-Fi stickers, calibration patterns, `doctor`, and a JSON config file.

[Unreleased]: https://github.com/kahwee/thermark/compare/v0.26.0...HEAD
[0.26.0]: https://github.com/kahwee/thermark/compare/v0.25.0...v0.26.0
[0.25.0]: https://github.com/kahwee/thermark/compare/v0.24.0...v0.25.0
[0.24.0]: https://github.com/kahwee/thermark/compare/v0.23.0...v0.24.0
[0.23.0]: https://github.com/kahwee/thermark/compare/v0.22.0...v0.23.0
[0.22.0]: https://github.com/kahwee/thermark/compare/v0.21.0...v0.22.0
[0.21.0]: https://github.com/kahwee/thermark/compare/v0.20.0...v0.21.0
[0.20.0]: https://github.com/kahwee/thermark/compare/v0.19.0...v0.20.0
[0.19.0]: https://github.com/kahwee/thermark/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/kahwee/thermark/compare/v0.17.0...v0.18.0
[0.17.0]: https://github.com/kahwee/thermark/compare/v0.16.0...v0.17.0
[0.16.0]: https://github.com/kahwee/thermark/compare/v0.15.0...v0.16.0
[0.15.0]: https://github.com/kahwee/thermark/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/kahwee/thermark/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/kahwee/thermark/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/kahwee/thermark/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/kahwee/thermark/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/kahwee/thermark/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/kahwee/thermark/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/kahwee/thermark/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/kahwee/thermark/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/kahwee/thermark/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/kahwee/thermark/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/kahwee/thermark/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/kahwee/thermark/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/kahwee/thermark/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kahwee/thermark/releases/tag/v0.1.0
