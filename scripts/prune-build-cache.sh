#!/usr/bin/env bash
# Reset this repository's generated artifacts at most once every two weeks.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="$root/target"
stamp="$target/.thermark-cache-start"
now="$(date +%s)"
last=0
if [[ -f "$stamp" ]]; then
    read -r last < "$stamp" || last=0
fi
case "$last" in ''|*[!0-9]*) last=0 ;; esac

# Adopt existing caches on first use; the next reset is due in 14 days.
if [[ "$last" == 0 ]]; then
    mkdir -p "$target"
    printf '%s\n' "$now" > "$stamp"
elif (( now - last >= 14 * 24 * 60 * 60 )); then
    echo 'Resetting Thermark build artifacts: the cache is at least 14 days old.'
    cargo clean --manifest-path "$root/Cargo.toml" --target-dir "$target"
    mkdir -p "$target"
    printf '%s\n' "$now" > "$stamp"
fi
