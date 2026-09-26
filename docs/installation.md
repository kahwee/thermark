# Install and update thermark

B1 with the `b1` task over Bluetooth LE is the only hardware-verified path.
USB serial and other supported profiles are experimental. A working installation
does not establish printer compatibility. Windows is not part of the tested
release matrix; the downloads below target macOS and Linux.

## Choose an installation

| Method | Requirements | Transports |
| --- | --- | --- |
| Homebrew tap | macOS, Apple Silicon or Intel | BLE |
| Release archive | macOS or Linux, matching CPU architecture | `ble` or `full` |
| Cargo | Rust 1.98+, native build prerequisites | BLE + serial by default |

### Homebrew on macOS

```sh
brew install kahwee/thermark/thermark
thermark --version
```

This is a project-maintained tap, not a Homebrew/core formula. It installs a
prebuilt BLE binary without compiling Rust. For experimental USB serial, choose
a `full` archive or the default Cargo installation.

### Cargo

```sh
cargo install thermark --locked
# Or choose BLE only:
cargo install thermark --locked --no-default-features --features ble
```

Cargo normally installs into `~/.cargo/bin`; ensure that directory is on `PATH`.
Run `rustc --version` before installing. If you use rustup, `rustup update stable`
updates that toolchain. On macOS, compiling also requires the Xcode command-line
tools (`xcode-select --install`). Prebuilt installations do not require them.

On Debian/Ubuntu, install build and Bluetooth dependencies before Cargo:

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config libdbus-1-dev bluez fonts-dejavu-core
```

Other distributions need equivalent compiler, pkg-config, D-Bus development,
BlueZ, and TrueType font packages. Running a BLE binary needs an available
Bluetooth adapter, D-Bus and the Bluetooth service; building in a container or
CI does not prove access to a physical adapter. Follow your distribution's
Bluetooth service and permission guidance rather than running thermark as root.

For rendering only, install without either transport:

```sh
cargo install thermark --locked --no-default-features
```

That build can render offline but cannot connect to a printer. Reinstall with
the desired features and `--force` when deliberately switching feature sets.

### Prebuilt archives

Download an archive and its matching `.sha256` file from
[GitHub Releases](https://github.com/kahwee/thermark/releases/latest).
Use `uname -m` to check your CPU: `arm64`/`aarch64` maps to `ARM64`, and
`x86_64` maps to `X64`. Select `macOS` or `Linux`, then `ble` or `full`.

Example for macOS Apple Silicon, version 0.33.0:

```sh
shasum -a 256 --check thermark-0.33.0-macOS-ARM64-ble.tar.gz.sha256
tar -xzf thermark-0.33.0-macOS-ARM64-ble.tar.gz
cd thermark-0.33.0-macOS-ARM64-ble
./thermark --version
mkdir -p "$HOME/.local/bin"
install -m 755 thermark "$HOME/.local/bin/thermark"
export PATH="$HOME/.local/bin:$PATH"
```

On Linux, choose the matching Linux filename and use `sha256sum --check`
instead of `shasum -a 256 --check`. Add the `PATH` export to your shell's startup
file to keep it across sessions. Archives are built on Ubuntu 24.04 and macOS 15;
older operating systems may need a source build. Checksums detect a damaged or
mismatched download; they are not independent signatures.

## Verify before printing

```sh
thermark --version
thermark fonts
thermark qr --url https://example.com --text 'Hello, label!' \
  --model b1 --label 50x30 --save hello-label.png --no-print
```

Open the PNG and scan its QR. If no font is found, install a supported system
font or add `--font /path/to/font.ttf`. This check uses no printer or paper.
For the physical B1 setup, follow the
[first-label guide](../README.md#install-and-print-your-first-label).

## Upgrade or uninstall

Use the same method you installed with:

| Method | Upgrade | Uninstall |
| --- | --- | --- |
| Homebrew | `brew update` then `brew upgrade kahwee/thermark/thermark` | `brew uninstall kahwee/thermark/thermark` |
| Cargo | Repeat your `cargo install thermark --locked` command, including feature flags | `cargo uninstall thermark` |
| Archive | Verify the new download, then replace the executable at the same location | Remove only the executable you installed |

If the version is unexpected, run `command -v thermark` to find which
installation your shell uses. Avoid mixing methods unintentionally. Uninstalling
an executable does not promise removal of your saved printer configuration.

## Get help

Run `thermark doctor --use-config --json` for a privacy-safe report. Include the
installation method, OS, architecture, version, and exact error in a
[GitHub issue](https://github.com/kahwee/thermark/issues). Review any screenshots
before sharing. Do not publish raw printer identities or real Wi-Fi credentials.
