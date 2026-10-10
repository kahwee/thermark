# First-print and batch hardware trial

This trial collects evidence from independent B1 owners. It does not establish
support for other models or USB. Use a build containing `setup` and `batch`;
these commands are currently on main and are not in the v0.34.0 release.

## Prepare

Install or build thermark following the [installation guide](installation.md).
For the commands below, work in a source checkout of main so the example CSV
and layout are available. Install that checkout's CLI rather than an older
release binary:

```sh
cargo install --path . --locked
thermark setup --help
thermark batch --help
```

Ensure Cargo's bin directory is on PATH; the two help commands confirm that
your shell finds the new build. Charge and wake the B1, load your media, and quit the vendor app.
Use your actual media size wherever the examples say 50x30.

Record OS/version, CPU architecture, `thermark --version`, and the tested Git
commit when using a source build. Start timing at installation, then record the
time of your first successful physical print. Note any step that needs help.

## One test label

```sh
thermark setup --label 50x30 --test-print
```

This selects a printer, identifies it, checks readiness, saves defaults, and
prints **one label** after checks pass. If several printers appear, choose the
right number in the terminal. A saved printer is reused; pass `--addr` to change
it. Scan the QR with your phone: it should open example.com.

Without `--test-print`, setup does not print. For a specific printer in a
noninteractive shell, supply `--addr '<exact name or id>' --label WxH`.
If font loading fails, pass `--font /path/to/a/system-font.ttf`.

## Three inventory labels

Preview first:

```sh
thermark batch --csv examples/batch/inventory.csv \
  --template examples/batch/inventory.json --label 50x30 \
  --preview-dir local/prints/trial-preview
```

Open all three previews and scan their QRs. Then print **three labels**:

```sh
thermark batch --csv examples/batch/inventory.csv \
  --template examples/batch/inventory.json --label 50x30 \
  --preview-dir local/prints/trial-run-1 --print
```

Check order, placement, readable text, and whether all three printed QRs scan
to the corresponding example.com/bins/A1, A2, and A3 pages. Their contents are
public demo data; the pages need not contain a real inventory system.
Read `journal.jsonl` and check that rows 1–3 each end with `confirmed`.

Optional: test Ctrl-C during a later batch with a new preview directory.
The active label may still finish. Inspect the output and follow the
[manual resume guide](batch.md#review-and-resume-a-stopped-run). Do not replay
an uncertain row without checking whether its label already printed.

## Report

Capture a privacy-safe diagnostic report:

```sh
thermark doctor --use-config --json > local/thermark-trial-report.json
```

Use the [hardware report form](https://github.com/kahwee/thermark/issues/new?template=hardware-report.yml)
and include:

- OS/version, CPU, build/version, printer model, and actual label size.
- Whether installation, preview, setup, the single print, and batch print worked.
- Time to first successful print and any step that required troubleshooting.
- Whether printed QRs scanned, text was legible, and all rows printed in order.
- Exact errors and the privacy-safe doctor report, if useful.

Omit printer IDs, serial numbers, private CSV content, and real Wi-Fi credentials.
Optional photos should show the public demo labels laid flat. Failed installs
and failed prints are useful evidence. After a week, note whether you used
thermark again, what you printed, and what made repeat use awkward.
