#!/usr/bin/env bash
# Release build using the existing cache. Extra arguments go to Cargo.
set -euo pipefail
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
    echo 'Usage: scripts/build.sh [cargo build options]'
    echo 'Builds a locked release in target/ using the existing cache.'
    exit 0
fi
cd "$(dirname "${BASH_SOURCE[0]}")/.."
cargo build --locked --release "$@" --target-dir "$PWD/target"
