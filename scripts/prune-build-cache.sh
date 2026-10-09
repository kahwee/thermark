#!/usr/bin/env bash
# Explicitly remove this repository's generated build artifacts.
set -euo pipefail
usage() {
    echo 'Usage: scripts/prune-build-cache.sh'
    echo "Remove this checkout's target/ artifacts. The next build recompiles them."
}
if [[ $# -eq 1 && ( $1 == --help || $1 == -h ) ]]; then
    usage
    exit 0
fi
if [[ $# -ne 0 ]]; then
    usage >&2
    exit 64
fi
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo clean --manifest-path "$root/Cargo.toml" --target-dir "$root/target"
