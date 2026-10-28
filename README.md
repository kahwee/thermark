# thermark

Local, scriptable monochrome labels: QR links, Wi-Fi stickers, inventory tags
and text. No vendor app, cloud or printing account required.

[Downloads](https://github.com/kahwee/thermark/releases/latest) ·
[Offline demo](https://kahwee.github.io/thermark/) · [Rust API](https://docs.rs/thermark)

**Hardware-verified: B1 with the b1 task over Bluetooth LE.** USB serial and
other printer profiles are experimental. Colour printing is unsupported.

## Install and print your first label

On macOS, install the prebuilt BLE binary for Apple Silicon or Intel:

```sh
brew install kahwee/thermark/thermark
```

On Linux, or macOS without Homebrew, use a [release archive](https://github.com/kahwee/thermark/releases/latest):
`ble` for Bluetooth, `full` for experimental USB serial. For source installation
with Rust 1.99+:

```sh
cargo install thermark --locked --version 0.36.1
```

See [installation and upgrades](docs/installation.md) for native prerequisites,
archive checksums, tagged-source installs and feature choices.

Turn on a B1, load 50×30 mm labels and close the vendor app:

```sh
thermark setup --label 50x30 --test-print
```

This saves defaults and prints an example.com QR label. Omit `--test-print` to
configure without printing. Allow terminal Bluetooth access when prompted;
[troubleshoot connection issues](docs/usage.md#macos-bluetooth-ownership) if needed.

## Preview without a printer

```sh
thermark qr --url https://example.com --text 'Hello, label!' \
  --model b1 --label 50x30 --save hello-label.png --no-print
```

This creates a PNG without connecting to hardware or saving printer config.
Scan the QR before printing. The embedded DejaVu Sans font needs no setup.

![Offline label preview](docs/demo/hello-label.png)

For CSV batches, layout templates, prepared jobs and interrupted-run recovery,
see [batch printing](docs/batch.md). Explore [eight label recipes](docs/recipes.md).
Keep real Wi-Fi labels and personal output in ignored `local/`.

## Support and documentation

`thermark tasks` lists profiles and verification status. Printing outside the
verified B1+b1+BLE path requires `--allow-experimental`; offline rendering does not.
B18 printing remains unresolved. Report hardware results through the
[printer report template](https://github.com/kahwee/thermark/issues/new?template=hardware-report.yml).

- [Commands, calibration and troubleshooting](docs/usage.md)
- [Hardware notes and evidence](docs/hardware-notes.md) · [hardware trial](docs/hardware-trial.md)
- [Development and tests](docs/development.md) · [architecture](docs/architecture.md)
- [Releases](docs/releasing.md) · [changelog](CHANGELOG.md) · [CI maintenance](.github/ACTIONS.md)
- [Contributing](CONTRIBUTING.md) · [agent guide](AGENTS.md)

[MIT licensed](LICENSE). Bundled fonts retain their [redistribution terms](LICENSE-FONTS).
