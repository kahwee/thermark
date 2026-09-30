# Printing, previews, and troubleshooting

## Print stickers

Always pass the physical label size. After `scan --save`, the printer address is
read from config.

Guest Wi-Fi:

```bash
THERMARK_WIFI_PASSWORD='your-password' \
  thermark wifi \
  --ssid "YourNetwork" \
  --label 50x30
```

Open networks do not need a password:

```bash
thermark wifi \
  --ssid "Cafe-Guest" \
  --security nopass \
  --label 50x30
```

URL with readable text:

```bash
thermark qr \
  --url "https://example.com/o/1042" \
  --text $'ORDER #1042\nPriority' \
  --font-name helvetica \
  --label 50x30
```

Plain text:

```bash
thermark text \
  --text $'FRAGILE\nthis way up' \
  --label 50x30
```

Existing artwork:

```bash
thermark print \
  -i local/prints/art.png \
  --label 50x30 \
  --no-fill \
  --margin 0 \
  -d 4
```

Personal artwork and real credentials belong under `local/`, which is ignored
by git. Committed files under [`fixtures/`](../fixtures/) contain public demo data
only.

### Which printer profile controls rendering

For an online `text`, `qr`, `wifi`, or `calibrate` command, thermark connects
and identifies the printer before converting the physical label size to pixels.
The detected profile's DPI and printhead width therefore determine the rendered
canvas. If a generated-label command also uses `--save`, the PNG is written
from that same detected-profile render before it is printed.

Hardware printing fails closed if the identity probe fails or reports an
unrecognized model. thermark will not render or send a job using provisional
profile geometry in that case.

`--no-print` remains connection-free. In that mode, generated labels use the
profile selected by `--model`, then the saved configuration, then the B1
default. Combine `--save <path> --no-print` when you want a wholly offline
render.

Online raw-image printing also lays out the image against the detected profile.
Its default 1 mm registration margin is converted at the detected DPI; an
explicitly saved pixel inset and `--full-bleed` remain exact.

## Preview and calibrate

Preview the exact bitmap for the selected profile without printing:

```bash
thermark print \
  -i local/prints/art.png \
  --label 50x30 \
  --preview local/preview.png
```

`print --preview` applies the selected threshold and dithering and writes the
final monochrome page for the configured or `--model` profile. If that profile
does not match hardware later detected by an online print, its geometry can
differ. Generated sticker commands use
`--save <path> --no-print` to inspect their composed artwork; those saved PNGs
can retain antialiasing that the printer later thresholds. Without
`--no-print`, `--save` waits until printer identification so the saved bitmap
and printed page share the same profile-sized render.

Check label placement on hardware:

```bash
thermark calibrate --label 50x30
thermark calibrate --boundary --label 50x30
```

### Label size and RFID

The printer's RFID response can describe the consumable type and remaining
label count, but it does not report dimensions in millimetres. Vendor software
may resolve an RFID barcode through its own catalogue; thermark has no cloud
catalogue. Pass `--label WxH` when media changes, or save the size with
`thermark config set --label WxH`.

## If a print looks wrong

1. Run `thermark info` and charge the printer if the battery is low.
2. Preview the bitmap to separate rendering problems from hardware problems.
3. Print the same bitmap twice. Inconsistent truncation points indicate power,
   not geometry.
4. Use `calibrate --boundary` only after the printer is charged.

A charged B1 reaches the full feed canvas. Its default 1 mm top/bottom inset is
registration margin, not a known unprintable band.

For connection failures on macOS, follow the
[Bluetooth ownership checks](#macos-bluetooth-ownership) above.


## macOS Bluetooth ownership

macOS can show a printer as **Connected** while thermark reports that it is not
discoverable. One likely cause is that another Bluetooth client—often a vendor
app or a system-managed session—still owns this printer's BLE GATT connection.
The same symptom can also mean the printer is asleep, out of range, or not
advertising, so treat ownership as a diagnosis to verify rather than a fact.

Before retrying, disconnect the printer in macOS Bluetooth settings, quit any
vendor label app, wake or power-cycle the printer, and then run:

```bash
thermark scan
thermark doctor --use-config
```

If `doctor` reports a matching `/dev/cu.…` endpoint, that supports the ownership
diagnosis, but proves neither ownership nor serial-protocol compatibility. Use
the endpoint with `--conn usb` only when the printer responds to the serial
protocol; otherwise release any competing Bluetooth session and use BLE
normally.


## Diagnostic reports

```sh
thermark doctor --use-config --json > thermark-report.json
```

The report excludes printer identities, RFID values, credentials, and local
paths. For private hardware investigation, `thermark identify --json` records
model/firmware/geometry/task; save that output under ignored `local/` and review
it before sharing. Use the exact advertising name from `scan`; `--fuzzy`
permits substring matching. macOS device addresses are UUIDs.
