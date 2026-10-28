# thermark

**Print labels without the vendor app.** Local, scriptable QR codes, Wi-Fi
stickers, inventory tags, and text for pocket thermal printers. No cloud or
account needed to print.

[Download for macOS or Linux](https://github.com/kahwee/thermark/releases/latest) ·
[View the offline demo](https://kahwee.github.io/thermark/) ·
[Report your printer](https://github.com/kahwee/thermark/issues/new?template=hardware-report.yml)

**Hardware-tested: B1 over Bluetooth LE.** USB and other printer profiles are
experimental. thermark prints monochrome labels; it does not support colour
printing.

## Install and print your first label

On macOS (Apple Silicon or Intel), install the prebuilt BLE binary:

```sh
brew install kahwee/thermark/thermark
```

For Linux, or macOS without Homebrew, use the
[prebuilt downloads](https://github.com/kahwee/thermark/releases/latest).
Choose `ble` for Bluetooth or `full` for experimental USB serial support.
No Rust compiler is needed for a prebuilt download.

For the current release from source, use Rust 1.99 or newer:

```sh
cargo install --git https://github.com/kahwee/thermark --tag v0.36.1 --locked
```

`cargo install thermark --locked` uses crates.io, whose version can lag GitHub.
The [installation guide](docs/installation.md) covers Linux prerequisites,
archive checksums, upgrades, and troubleshooting. Run `thermark --version` to
check the executable your shell found.

Turn on your B1, load 50×30 mm labels, and quit the vendor app. Then:

```sh
thermark setup --label 50x30 --test-print
```

Setup selects the printer, checks readiness, saves defaults, then prints one
example.com QR label. Omit `--test-print` to configure without printing. Allow
your terminal Bluetooth access when prompted; see [connection troubleshooting](docs/usage.md#macos-bluetooth-ownership)
if discovery fails.

### Print a CSV batch

Create a CSV in your working directory; a source checkout is not required:

```sh
cat > labels.csv <<'CSV'
text,url
Cables,https://example.com/bins/A1
Adapters,https://example.com/bins/A2
CSV
thermark batch --csv labels.csv --model b1 --label 50x30 \
  --preview-dir inventory-preview
```

This prepares monochrome PNGs and a manifest without connecting to a printer.
Use a fresh preview directory for each batch. Inspect the PNGs, then print the
exact prepared pages with your configured printer:

```sh
thermark job verify --job inventory-preview
thermark job print --job inventory-preview --label 50x30
thermark job status --job inventory-preview
```

After a stopped run, inspect the printed labels and journal before using
`thermark job resume --job inventory-preview --label 50x30`. Confirmed rows
are skipped; uncertain output requires physical review. See
[CSV layouts and recovery](docs/batch.md) for templates and resume controls.

### Try it without a printer

```sh
thermark qr --url "https://example.com" --text "Hello, label!" \
  --model b1 --label 50x30 --save hello-label.png --no-print
```

This saves a real PNG locally without connecting to hardware. Scan its QR or
inspect the layout before spending a label.

| Guest Wi-Fi | Inventory | URL + text |
| --- | --- | --- |
| ![Demo Wi-Fi label with fake credentials](docs/demo/recipes/guest-wifi.png) | ![Demo inventory label](docs/demo/recipes/inventory.png) | ![Demo URL label](docs/demo/hello-label.png) |

These are rendered previews using public demo data, not photographs of prints.

Explore [eight copy-and-paste label recipes](docs/recipes.md): Wi-Fi, inventory,
manuals, return instructions, badges, storage, packing, and batch labels.
Render the whole gallery with `sh examples/render-recipes.sh` from a source checkout.

## Support

| Model family / path | Status |
|---|---|
| B1 over BLE | Hardware-tested |
| B1 over USB serial | Experimental; implemented and mock-tested |
| B1 Pro, B21 Pro, D11, D11_H, D110 | Experimental monochrome profiles |
| B18 | Geometry known; print task unresolved |

Printing with any profile/task/connection combination other than B1+B1 over
BLE requires `--allow-experimental`; offline previews and saved renders do not.
Multi-colour printheads and colour raster protocols are out of scope.

## Common tasks

| Goal | Command |
| --- | --- |
| Configure a B1 without printing | `thermark setup --label 50x30` |
| Check the saved printer | `thermark doctor --use-config --json` |
| List profiles and verification status | `thermark tasks` |
| Find a command's options | `thermark <command> --help` |
| Upgrade the Homebrew installation | `brew update` then `brew upgrade kahwee/thermark/thermark` |

Previews use the embedded DejaVu Sans font. Pass `--font /path/to/font.ttf` for
a different font. Use public demo credentials in shared Wi-Fi previews; real
labels belong outside the repository or under ignored `local/prints/`.

## Documentation

- [Installation, upgrades, and prerequisites](docs/installation.md)
- [Printing, previews, calibration, and troubleshooting](docs/usage.md)
- [Builds, tests, benchmarks, and security checks](docs/development.md)
- [Release checklist and distribution updates](docs/releasing.md)
- [CSV batches and reusable layouts](docs/batch.md)
- [First-print and batch hardware trial](docs/hardware-trial.md)
- [Contributing and hardware reports](CONTRIBUTING.md)
- [Hardware notes](docs/hardware-notes.md)
- [Architecture and prepared jobs](docs/architecture.md)
- [Library API](https://docs.rs/thermark) and [release history](CHANGELOG.md)
- [Agent guidance](AGENTS.md)

## License

MIT

## CI maintenance

[GitHub Actions maintenance](.github/ACTIONS.md) covers workflows, parallel checks, action versions, and weekly updates.
