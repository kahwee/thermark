#!/bin/sh
# Render public demo labels only; never connects to a printer.
set -eu
output_dir=${1:-local/prints/recipes}
thermark_bin=${THERMARK_BIN:-thermark}
mkdir -p "$output_dir"
render() {
  name=$1
  shift
  if [ -n "${THERMARK_FONT:-}" ]; then
    "$thermark_bin" "$@" --font "$THERMARK_FONT" --model b1 --label 50x30 --save "$output_dir/$name.png" --no-print
  else
    "$thermark_bin" "$@" --model b1 --label 50x30 --save "$output_dir/$name.png" --no-print
  fi
}
render guest-wifi wifi --ssid "Demo-Guest" --password "demo-only-1234"
render inventory qr --url "https://example.com/bins/A3" --text "BIN A3\nCables"
render equipment qr --url "https://example.com/manuals/drill" --text "DRILL 01\nManual"
render return qr --url "https://example.com/return/42" --text "KIT 42\nReturn here"
render badge text --text "ALEX\nVolunteer"
render storage text --text "M3 BOLTS\nDrawer 04"
render packing text --text "FRAGILE\nThis way up"
render batch text --text "BATCH 024\n2026-09-30"
