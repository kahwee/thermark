# Changelog

All notable changes to **thermark** are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the crate is `0.x`, the public API may change in any minor release; each
such change is listed under **Changed** with the old and new spelling.

## [Unreleased]

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

[Unreleased]: https://github.com/kahwee/thermark/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/kahwee/thermark/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kahwee/thermark/releases/tag/v0.1.0
