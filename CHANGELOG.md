# Changelog

All notable changes to **thermark** are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the crate is `0.x`, the public API may change in any minor release; each
such change is listed under **Changed** with the old and new spelling.

## [Unreleased]

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

[Unreleased]: https://github.com/kahwee/thermark/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/kahwee/thermark/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/kahwee/thermark/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/kahwee/thermark/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/kahwee/thermark/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kahwee/thermark/releases/tag/v0.1.0
