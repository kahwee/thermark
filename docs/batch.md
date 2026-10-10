# CSV batches and reusable layouts

`thermark batch` makes one label per CSV data row. It renders offline by
default. Adding `--print` explicitly sends the batch to a printer after every
row has rendered and every preview has been saved. No shell loop is needed.

## Preview an inventory batch

From a source checkout:

```sh
thermark batch --csv examples/batch/inventory.csv \
  --template examples/batch/inventory.json \
  --preview-dir local/prints/inventory-preview
```

Open `label-00001.png` through `label-00003.png` and scan their QRs. The
previews contain the final monochrome pixels. The offline profile comes from
`--model`, saved configuration, or B1. If hardware later identifies as a
different profile, print-run previews use that detected profile instead.

After setup, print **three labels**:

```sh
thermark batch --csv examples/batch/inventory.csv \
  --template examples/batch/inventory.json \
  --preview-dir local/prints/inventory-run-1 --print
```

Use a **fresh directory for every run**. Existing directories are rejected,
so previews and receipts from earlier runs cannot be overwritten. Replace
50x30 with your actual media size using `--label WxH`.

## CSV format

Use UTF-8 CSV with exactly one `text` column and, for QR labels, one `url`
column. Column order is flexible. Other or duplicate columns are errors.
Each row makes one label; row numbers start at 1 after the header.

```csv
text,url
"BIN A1\nCables",https://example.com/bins/A1
"BIN A2\nAdapters",https://example.com/bins/A2
```

With only `text`, the batch makes text labels:

```sh
thermark batch --csv examples/batch/badges.csv --label 50x30 \
  --preview-dir local/prints/badges-preview
```

Quoted commas, doubled quotes, and quoted multiline fields are supported.
The literal sequence `\n` also makes a line break, matching the text and QR
commands. Text must be nonempty; QR rows also need a nonempty URL/payload.
Keep private CSVs and previews in your own private directory or under ignored
`local/`; previews contain the label content. The journal omits that content.

Inputs are limited to 8 MiB each, 1,000 rows, and 64 MiB of rendered grayscale
pages per batch. Split larger jobs. A row that cannot fit or render stops the
whole batch before any label is sent. A preview save failure also sends no
labels, although it may leave an incomplete output directory.

## Reusable JSON layouts

A template stores layout settings, while CSV holds the changing content:

```json
{
  "kind": "text",
  "label": "50x30",
  "align": "left",
  "font_size": 24,
  "border": false,
  "density": 4
}
```

All fields are optional. Supported fields:

| Field | Values / default |
| --- | --- |
| `kind` | `text` or `qr`; inferred from CSV headers if omitted |
| `label` | Physical mm, such as `50x30`; saved/default size if omitted |
| `align` | `left`, `center`, `right`; `center` for text labels |
| `text_side` | `left`, `right`; `right` for QR labels |
| `font` | `.ttf` / `.ttc` path, relative to the template directory |
| `font_name` | Named system font, as in `thermark fonts` |
| `font_size` | Finite positive pixel size; auto-fit when omitted |
| `border` | Boolean; `false` |
| `density` | Integer 1–5; `4` |

Unknown fields are errors. `--label`, `--density`, and font flags override
matching template settings. An explicit `--font` or `--font-name` replaces
the template's font selector. Fonts must be available on the rendering host.
A `text` template rejects a CSV containing `url`, rather than discarding QR
content; a `qr` template requires that column.

## Review and resume a stopped run

Print runs write `journal.jsonl`. Before each label can act, thermark writes
and syncs an `uncertain` entry; after printer completion is confirmed, it
writes and syncs a `confirmed` entry. A fault stops subsequent labels and
releases the connection. Ctrl-C releases the local connection; it does not
send a printer-side cancellation command, so the active label may still finish.

Read the **last complete entry for each row**:

- `confirmed`: thermark received a successful completion for that row.
- `uncertain`: it may have printed, partially printed, or not printed.
- No row entry: thermark did not send that row in this run.

If a crash leaves a partial final JSON line, ignore that line and treat any
row without a complete confirmation as uncertain. Inspect physical labels
before deciding where to resume. thermark never automatically replays a job.

For example, if rows 1 and 2 are physically complete and row 3 is missing:

```sh
thermark batch --csv examples/batch/inventory.csv \
  --template examples/batch/inventory.json --start-at 3 \
  --preview-dir local/prints/inventory-run-2 --print
```

Keep the original CSV, layout, media, and relevant settings unchanged. `--start-at`
is an explicit choice, not a receipt-based resume engine; thermark does not
compare input files with earlier runs. All rows are validated and previewed
again, but only the selected row and those after it are sent. Each run keeps
its own journal. `manifest.json` describes geometry and counts; it does not
claim that labels printed. A confirmed protocol result still needs physical
inspection to establish print quality and QR scanability.
