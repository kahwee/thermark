# Architecture and prepared jobs

Thermark is one crate with shared library workflows and a thin CLI.

| Owner | Responsibility |
| --- | --- |
| `profile` and `print_task` | Physical geometry, support evidence, wire-task selection |
| `session/connection` | Native or caller-provided transport acquisition |
| `session/policy` | Known identity and explicit experimental-path permission |
| `session` | Identified sessions, job execution helpers, explicit shutdown |
| `batch/input` | Bounded CSV and template parsing; configuration precedence |
| `batch` | Render once with a shared font, spool pages, execute verified jobs |
| `batch/journal` | Typed durable outcomes, print-stage records, process locking, recovery |
| `prepared` | Versioned manifest, content hashes, geometry and page validation |
| `label/{qr,text,calibration}` | Composition; shared content and QR geometry stay in `label` |
| `font` | Embedded default, glyph coverage, bidi/script shaping, bounded layout cache |
| `doctor/{report,probe}` | Report presentation and privacy filtering; native readiness probes |
| `testing` | Synthetic transcript replay and deterministic transport faults |
| `cli` | Arguments, environment/config resolution, output, command dispatch |

Use `thermark::session` for connections and `thermark::batch` for preparation
and execution. See `examples/prepared_job.rs` for a mock-only library example.
Callers supply connection/pacing options explicitly. Call `Session::finish`
after success or failure; cancelling a future drops its transport, so custom
transports must implement their own cleanup.

## Prepared artifact contract

Schema 2 stores the registered model/DPI, selected print task, density, pixel
geometry, resolved font bytes/face identity, input/layout hashes, and a hash for
each final monochrome PNG. The job identifier hashes the canonical manifest
with its identifier cleared. No local font path or printer identity is stored.
Pages contain label content and must be kept private when that content is private.
Hashes detect accidental edits; they do not authenticate who approved an artifact.

Preparation loads one font and renders one page at a time into a temporary
sibling directory. It publishes the directory only after every page succeeds.
Input/row limits remain 8 MiB and 1,000 labels; a page has a 64 MiB memory limit.
Execution verifies the complete job and connected geometry/task before the
first print, then each page again before consuming it. Rendering settings are
never reapplied.
A connected `batch --print` freezes the detected firmware-sensitive task.
An offline job uses the model default or your explicit task selection; printing
retains automatic firmware detection unless `--task` is explicitly supplied.
A differing detected task is rejected before printing. Physical media must
match the dimensions declared by the operator.

An OS file lock excludes concurrent execution of the same directory. Each
attempt gets a new append-only, synced journal. `uncertain` is durable before
hardware can act; `confirmed` follows protocol completion. Stage entries explain
where execution reached. A partial final line is not a receipt. Resume skips
confirmed rows and requires an explicit physical-review acknowledgement for
uncertain output or a partial tail. It never claims exactly-once physical printing.

## Fonts

DejaVu Sans is embedded and redistributed under `LICENSE-FONTS`. This makes the
default independent of host fonts. Explicit `--font` and `--font-name` selectors
continue to work. `LabelFont::source_path()` returns `None` for the embedded
font; the existing `path()` returns a display-only source name in that case.
Missing characters fail before label pixels are drawn; use a
font covering the requested script. This is a single-font renderer, not automatic
font fallback. Simple Latin placement retains its previous metrics; combining
text and other scripts use bidi runs, OpenType shaping and grapheme-safe wrapping.
Layout-cache size is bounded; it is reused across batch rows.

Prepare schema 1 previews again before using `job print` or `job resume`.
Use typed `JobReport` or `job status --json` to consume outcomes; journal line
counts vary with print stages and attempts.

## Validation boundaries

Golden renders, QR decoding, replay and mock faults verify offline behavior.
`fixtures/protocol/density-fragmented.json` is synthetic public demo data, not a
capture from hardware. Fault injection covers failures after potentially accepted
prefixes; it does not simulate every firmware or radio behavior. Use the hardware
trial tool and physical measurements before changing support evidence or claiming
better throughput or reliability than another implementation.
