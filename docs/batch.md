# CSV batches and prepared jobs

`thermark batch` renders one monochrome label per CSV row without connecting.
From a source checkout, prepare and inspect the three inventory previews:

```sh
thermark batch --csv examples/batch/inventory.csv \
  --template examples/batch/inventory.json \
  --preview-dir local/prints/inventory-preview
thermark job verify --job local/prints/inventory-preview
```

Use a fresh output directory. Preparation publishes it only after every page
renders and saves successfully. Keep private inputs and previews under ignored
`local/` or another private directory.

## Print and recover

After inspecting the PNGs and loading matching media, print the saved pages:

```sh
thermark job print --job local/prints/inventory-preview --label 50x30
thermark job status --job local/prints/inventory-preview
# Machine-readable outcomes:
thermark job status --job local/prints/inventory-preview --json
```

The connected model, DPI, print task and declared media must match the job.
Execution verifies every page before printing and each page again before use.
`job verify` and `job status` work offline without reading printer configuration.
For immediate preparation and printing, `batch --print` performs both steps.

Each attempt records synced journals: `uncertain` before hardware can act,
`confirmed` after successful protocol completion, and intermediate print stages.
Confirmation does not establish physical quality or QR scanability. Ctrl-C
releases the connection but the active label may still finish.

Inspect physical output before resuming. For example, if row 3 is missing:

```sh
thermark job resume --job local/prints/inventory-preview --label 50x30 \
  --start-at 3 --acknowledge-uncertain
```

Confirmed rows are skipped. Uncertain rows at or after the selected row require
`--acknowledge-uncertain`; an incomplete journal tail also requires it. Earlier
rows remain skipped by your explicit selection. With no uncertainty, omit the
flag. Resume adds a numbered journal and `job status` combines all attempts.
Keep journals with the original job. Concurrent execution of that directory is
rejected. Older schema 1 preview directories must be prepared again.

## CSV format and limits

Use UTF-8 CSV with exactly one `text` column and an optional `url` column.
Rows with `url` produce QR labels; otherwise they produce text labels:

```csv
text,url
"BIN A1\nCables",https://example.com/bins/A1
"BIN A2\nAdapters",https://example.com/bins/A2
```

Quoted commas, doubled quotes, multiline fields and literal `\n` line breaks
are supported. Text and QR payloads must be nonempty. Unknown or duplicate
columns fail. Inputs are limited to 8 MiB each, 1,000 rows, and 64 MiB per rendered
grayscale page. Split larger jobs. Any invalid row stops preparation before printing.

## JSON layouts

Templates hold reusable settings; CSV holds content. For example:

```json
{"kind":"text","label":"50x30","align":"left","font_size":24,"density":4}
```

All fields are optional; unknown fields fail:

| Field | Values / default |
| --- | --- |
| `kind` | `text` or `qr`; inferred from headers |
| `label` | Physical mm, such as `50x30`; saved/default size |
| `align` | `left`, `center`, `right`; `center` for text |
| `text_side` | `left`, `right`; `right` for QR |
| `font` | `.ttf` / `.ttc` path, relative to the template |
| `font_name` | Named system font from `thermark fonts` |
| `font_size` | Finite positive pixels; auto-fit when omitted |
| `border` | Boolean; `false` |
| `density` | Integer 1–5; `4` |

CLI label, density and font flags override the template. The default font is
embedded DejaVu Sans; custom fonts must be available when preparing. A text
template rejects `url` columns; a QR template requires one. Offline geometry
comes from `--model`, saved configuration, or B1; `batch --print` uses the
detected printer profile.

See [architecture](architecture.md) for library entry points and artifact contracts.
