# Hardware and protocol evidence

These observations come from the owned B1 over BLE. USB is mock-tested;
other profiles remain experimental. Commands and troubleshooting are in
[usage](usage.md); repeatable physical measurements are in [hardware trials](hardware-trial.md).

## B1 geometry and power

- Profile: 8 px/mm, maximum width 384 px; 50×30 mm renders as 384×240 px.
- A charged B1 printed the final boundary bar at rows 232–239. Its 1 mm
  top/bottom safe area is a registration margin, not a measured unprintable band.
- At battery level 1/4, dense pages stopped at varying positions. Charging
  changed the apparent cutoff by roughly 7 mm. This explains those observations;
  it does not rule out other faults on future prints.
- A curled-label photograph produced contradictory measurements differing by
  5 mm. Lay paper flat and read printed boundary numerals instead of estimating.

Charge before measuring, compare repeated prints of the same bitmap and use
`calibrate --boundary`. Change geometry from repeated physical evidence, not a
single clipped label. Tests should pin observable ink bounds rather than an
unverified theory about unreachable edges.

## Transport and protocol

| Item | Current behavior |
| --- | --- |
| BLE service | `e7810a71-73ae-499d-8c15-faa9aef0c3f2` |
| Characteristic | `bef8d6c9-9c21-4c9e-b632-bd58c1009f9f` |
| I/O | Write without response; notifications carry replies |
| Selection | Exact advertising name/id by default; `--fuzzy` is explicit; printer GATT UUIDs required |
| B1 task | 7-byte PrintStart, 6-byte SetPageSize: rows, columns, copies |
| Geometry | Rows follow feed height, columns follow width; registered profile sets width/DPI |
| Fault reply | `0xDB` first byte is a printer fault, such as cover open or no paper |
| Info reply | Command `0x40 + key` |
| Cancellation | Ctrl-C releases the local connection; it does not send printer-side cancellation |

Command definitions live in `src/protocol.rs`; pacing in `src/printer/pacing.rs`.
Acknowledged writes were tried on the owned B1 without a measurable improvement.
Do not infer radio behavior or throughput from mock transports. SIGKILL can
leave a link held until the printer times out.

## Implemented and omitted behavior

| Behavior | Status / constraint |
| --- | --- |
| Row-repeat coalescing | Implemented; split runs at the one-byte limit of 255 |
| `PrinterCheckLine` (`0x86`) | Not sent; optional in observed implementations, not a demonstrated reliability gap |
| Sparse indexed rows (`0x83`) | Named in the command enum but not emitted. Other clients restrict it to ≤6 black pixels because of a reported firmware quirk |
| Bitmap black-pixel counts | Three zero bytes sent; owned B1 prints with them. Computing counts remains an unverified alternative |
| D11/D110 orientation | Narrow-side-first authoring (for example `12x40`); no automatic model rotation |
| PrintStatus (`0xa3`) | Parse page/print/feed progress and fault at offset 6 only in the 10-byte response |
| PrintEnd | A false result is refusal/not-ready; retry until accepted or the bounded attempt limit fails |
| Positioning calibration | Not exposed; values 1/2 ejected roughly 15 cm on the owned B1 |
| Consumable type | Print setup uses `set_label_type(1)`; automatic RFID selection needs continuous-stock hardware verification |

Wire page height is bounded by the `u16` row count, not a catalogue of preset
sizes. Allocation and layout limits still apply. Larger dense pages can expose
weak-battery behavior. Neither command definitions nor tests expand hardware support.

## Choose the evidence

| Question | Check |
| --- | --- |
| Final monochrome pixels | `thermark print -i art.png --label 50x30 --preview out.png` |
| Unexpected output change | `scripts/compare-render.sh <ref>` or golden tests |
| Ink placement | `cargo test --locked --test label_placement` |
| Physical feed boundary | Charged-printer `calibrate --boundary`; costs one label |
| Quality, scanability, repeatability | Actual prints and the hardware trial report |

Public `fixtures/` use demo data and are covered by `tests/fixtures_readme.rs`.
Keep private identities, artwork and real credentials under ignored `local/`.
