#!/usr/bin/env bash
# Release build with bounded cache retention. Extra arguments go to Cargo.
set -euo pipefail
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
    echo 'Usage: scripts/build.sh [cargo build options]'
    echo 'Builds a locked release in target/; resets its cache every 14 days.'
    exit 0
fi
cd "$(dirname "${BASH_SOURCE[0]}")/.."
bash scripts/prune-build-cache.sh
cargo build --locked --release "$@" --target-dir "$PWD/target"
