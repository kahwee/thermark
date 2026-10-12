# Printing and troubleshooting

See [recipes](recipes.md) for public demo labels and [batches](batch.md) for
preparation, saved-page printing and recovery.

## Setup and print

```sh
thermark setup --label 50x30
# Explicitly print one test label after readiness checks:
thermark setup --label 50x30 --test-print
```

Setup selects and identifies a printer, checks readiness, then saves defaults.
It resets registration measurements when changing printers. Pass `--addr` for
an exact name/id; setup rejects `--fuzzy`. Without `--label`, setup prompts for
media size. Noninteractive setup needs both `--addr` and `--label`.

Use your actual media size. Once configured:

```sh
thermark qr --url https://example.com/o/1042 --text 'ORDER #1042\nPriority' --label 50x30
thermark text --text 'FRAGILE\nThis way up' --label 50x30
THERMARK_WIFI_PASSWORD='your-password' thermark wifi --ssid YourNetwork --label 50x30
thermark wifi --ssid Cafe-Guest --security nopass --label 50x30
thermark print -i local/prints/art.png --label 50x30 --no-fill --margin 0 -d 4
```

The default font is embedded DejaVu Sans. Use `--font` or `--font-name` to change
it. Text auto-fits; explicit `--font-size` is finite and positive, rendered at
6–96 px. Missing glyphs or text that cannot fit fail before saving or printing.
Shorten text, enlarge media or omit the fixed font size.

B1 + b1 task + BLE is the hardware-tested path. Other print paths require
`--allow-experimental`. Keep private artwork and Wi-Fi labels under ignored `local/`.

## Preview and profile selection

```sh
thermark qr --url https://example.com --text 'Hello, label!' \
  --model b1 --label 50x30 --save local/hello.png --no-print
thermark print -i local/prints/art.png --model b1 --label 50x30 --preview local/preview.png
```

Both commands work offline. The profile comes from `--model`, saved config, or
B1. `print --preview` saves the final thresholded/dithered monochrome page;
sticker `--save` output can retain antialiasing. Prepared batch pages are final
monochrome PNGs and `job print` verifies them without rerendering.

Online printing identifies hardware first and uses its DPI and printhead width.
Unknown or failed identification stops printing. Online sticker `--save` uses
the same detected-profile render as the print; an offline preview can differ if
its chosen profile differs from hardware. Raw-image printing also uses detected
geometry, with a default 1 mm registration margin unless overridden.

## Label size and RFID

RFID describes consumable type/count, not dimensions in millimetres. Pass
`--label WxH` when media changes or save it with `thermark config set --label WxH`.
Thermark has no vendor cloud catalogue to resolve a roll's size.

## If a print looks wrong

1. Check `thermark info` and charge the printer.
2. Inspect the preview, then print the same bitmap twice.
3. Compare truncation positions before changing geometry; persistent variation
   can also be a transport or firmware fault.
4. On a charged printer, use `thermark calibrate --boundary --label 50x30`.

The owned charged B1 reaches the full feed canvas. Its 1 mm top/bottom inset is
a registration margin. See [hardware evidence](hardware-notes.md).

## macOS Bluetooth ownership

A printer shown as Connected but absent from scanning may have a competing
GATT session, be asleep, out of range or not advertising. Disconnect it in
Bluetooth settings, quit vendor apps, wake/power-cycle it, then retry:

```sh
thermark scan
thermark doctor --use-config
```

A matching `/dev/cu.…` endpoint supports the competing-session diagnosis but
proves neither ownership nor serial compatibility. Use `--conn usb` only if the
printer accepts the serial protocol. Select the full advertising name or id;
`--fuzzy` deliberately allows substring matching. macOS device ids are UUIDs.

## Diagnostic reports

```sh
thermark doctor --use-config --json > local/thermark-report.json
```

Doctor reports omit identities, RFID values, credentials and local paths.
`thermark identify --json` contains model/firmware/geometry/task details for
private investigation; review it before sharing. Create `local/` if needed.
