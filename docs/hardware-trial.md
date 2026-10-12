# Hardware trials

B1 with the `b1` task over BLE is the only hardware-verified print path.
Offline previews and protocol confirmations do not establish physical quality.

## First print

[Install thermark](installation.md), charge and wake the printer, load matching
media, and quit the vendor app. Replace 50x30 with your actual label size:

```sh
thermark setup --label 50x30 --test-print
```

Setup identifies the printer, checks readiness, saves defaults and prints one
example.com QR label. Scan it with your phone. Record OS/CPU, build version,
installation method, time to first successful print and any troubleshooting.
Without `--test-print`, setup sends no label.

## Repeatable measurements

From a source checkout with the installed CLI on PATH, prepare public sparse
text (3 labels), QR (3), repeated-batch (30) and dense-QR (10) jobs offline:

```sh
python3 scripts/hardware-trial.py --output local/trial-1 prepare --label 50x30
thermark job verify --job local/trial-1/qr
thermark job print --job local/trial-1/qr --label 50x30
thermark job status --job local/trial-1/qr
```

Inspect previews before printing. Use a fresh trial directory for each
build/media/environment. Pass `prepare --commit <full-source-SHA>` when known.
Keep charged and low-battery results separate; record interrupted/reconnected
runs with `record --condition interrupted` or `reconnected`. Inspect physical
output and follow [batch recovery](batch.md#print-and-recover) before retrying.

Count complete physical labels, scan each QR, and enter actual measurements:

```sh
python3 scripts/hardware-trial.py --output local/trial-1 record \
  --case qr --printed 3 --scanned 3 --seconds 12.5 --battery charged
python3 scripts/hardware-trial.py --output local/trial-1 report
```

The example duration is illustrative. Sparse text has no QR: use `--scanned 0`.
Reports distinguish offline preparation from operator-reported physical trials;
unmeasured durations stay empty. Repeat on macOS and Linux where available.

## Report results

```sh
thermark doctor --use-config --json > local/thermark-trial-report.json
```

Use the [hardware report form](https://github.com/kahwee/thermark/issues/new?template=hardware-report.yml).
Include OS/CPU, build, model, media size, steps that worked or failed, exact
errors, legibility, QR scans and row order. Failed installs and prints are useful.
After a week, report repeat use and remaining friction.

Keep printer IDs, serial numbers, private labels and Wi-Fi credentials out of
reports. Photos should show only public demo labels. Update support evidence
only from physical results, never from the trial script's offline preparation.
