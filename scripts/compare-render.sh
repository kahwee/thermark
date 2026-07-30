#!/usr/bin/env bash
# Compare label renders between a git ref and the working tree.
#
#   scripts/compare-render.sh v0.11.0
#
# Builds <ref> in a throwaway worktree, renders the same labels through both
# binaries, and byte-compares the PNGs. Answers "did this change alter any
# output?" without a printer or a wasted label.
#
# Use this for changes that are *meant* to be behaviour-preserving. For changes
# that are meant to alter output, `cargo test --test golden` reviews the diff
# instead, and a real print confirms the intent.
#
# The golden tests cover the library renderers; this covers the CLI end to end,
# including flag handling, and works against refs predating the golden suite.
set -euo pipefail

REF="${1:-}"
if [[ -z "$REF" ]]; then
    echo "usage: $(basename "$0") <git-ref>" >&2
    exit 64
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if ! git rev-parse --verify --quiet "$REF^{commit}" >/dev/null; then
    echo "error: '$REF' is not a git ref in this repository" >&2
    exit 64
fi

WORK="$(mktemp -d)"
WORKTREE="$WORK/ref"
BEFORE="$WORK/before"
AFTER="$WORK/after"
mkdir -p "$BEFORE" "$AFTER"

cleanup() {
    git worktree remove --force "$WORKTREE" >/dev/null 2>&1 || true
    rm -rf "$WORK"
}
trap cleanup EXIT

# Deterministic artwork, so `print` is exercised without depending on any file
# that may not exist at the older ref.
ART="$WORK/art.png"
python3 - "$ART" <<'PY'
import struct, sys, zlib
w = h = 200
rows = bytearray()
for y in range(h):
    rows.append(0)  # filter: none
    for x in range(w):
        on_border = 40 <= y < 160 and 40 <= x < 160 and (
            y < 46 or y >= 154 or x < 46 or x >= 154)
        rows.append(0 if on_border else 255)

def chunk(tag, data):
    return (struct.pack(">I", len(data)) + tag + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))

png = (b"\x89PNG\r\n\x1a\n"
       + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0))
       + chunk(b"IDAT", zlib.compress(bytes(rows)))
       + chunk(b"IEND", b""))
open(sys.argv[1], "wb").write(png)
PY

render() {
    local bin="$1" outdir="$2"
    "$bin" text --text $'THERMARK\nbulldozer crew\n#1' --label 50x30 \
        --no-print --save "$outdir/text.png" >/dev/null 2>&1
    "$bin" qr --url "https://github.com/kahwee/thermark" \
        --text $'ORDER 1042\nShip Friday' --label 50x30 \
        --no-print --save "$outdir/qr.png" >/dev/null 2>&1
    THERMARK_WIFI_PASSWORD='s3cret-password' "$bin" wifi --ssid "Cafe-Guest" \
        --label 50x30 --no-print --save "$outdir/wifi.png" >/dev/null 2>&1
    "$bin" print -i "$ART" --label 50x30 --no-fill \
        --preview "$outdir/art_contain.png" >/dev/null 2>&1 || true
    "$bin" print -i "$ART" --label 50x30 \
        --preview "$outdir/art_fill.png" >/dev/null 2>&1 || true
}

echo "building working tree..."
cargo build --quiet
render "$REPO_ROOT/target/debug/thermark" "$AFTER"

echo "building $REF..."
git worktree add --detach "$WORKTREE" "$REF" >/dev/null 2>&1
cargo build --quiet --manifest-path "$WORKTREE/Cargo.toml"
render "$WORKTREE/target/debug/thermark" "$BEFORE"

echo
printf '%-16s %s\n' "RENDER" "$REF vs working tree"
status=0
found=0
for f in text qr wifi art_contain art_fill; do
    a="$BEFORE/$f.png"
    b="$AFTER/$f.png"
    if [[ ! -f "$a" && ! -f "$b" ]]; then
        printf '%-16s %s\n' "$f" "skipped (neither version produced it)"
        continue
    fi
    found=1
    if [[ ! -f "$a" ]]; then
        printf '%-16s %s\n' "$f" "NEW (absent at $REF)"
        status=1
    elif [[ ! -f "$b" ]]; then
        printf '%-16s %s\n' "$f" "GONE (absent in working tree)"
        status=1
    elif cmp -s "$a" "$b"; then
        printf '%-16s %s\n' "$f" "identical"
    else
        printf '%-16s %s\n' "$f" "DIFFERS"
        status=1
    fi
done

if [[ "$found" -eq 0 ]]; then
    echo "error: neither version rendered anything — check the CLI flags in this script" >&2
    exit 70
fi

echo
if [[ "$status" -eq 0 ]]; then
    echo "No output changed. Behaviour-preserving."
else
    KEEP="$REPO_ROOT/target/compare-render"
    rm -rf "$KEEP"
    mkdir -p "$KEEP"
    cp -R "$BEFORE" "$KEEP/before"
    cp -R "$AFTER" "$KEEP/after"
    echo "Output changed. Images kept for inspection:"
    echo "  $KEEP/before  ($REF)"
    echo "  $KEEP/after   (working tree)"
    echo "If the change was intended, confirm with a print."
fi
exit "$status"
