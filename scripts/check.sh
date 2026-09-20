#!/usr/bin/env bash
# Shared local/CI validation. No printer access or saved-config changes.
set -euo pipefail

usage() {
    cat <<'HELP'
Usage: scripts/check.sh [rust|features|render|all]

  rust      Formatting, Clippy, and the full default-feature suite (default)
  features  Library feature matrix and BLE-only / serial-only binary builds
  render    Golden renders, public fixtures, and label placement
  all       rust + features; matches CI without rerunning rendering tests

Runs from any working directory. Requires Cargo, rustfmt, and Clippy; Linux
BLE builds also need libdbus-1-dev and pkg-config. No printer is required.
Golden acceptance is separate: UPDATE_GOLDEN=1 cargo test --test golden.
HELP
}

mode="${1:-rust}"
if [[ $# -gt 1 ]]; then
    usage >&2
    exit 64
fi
case "$mode" in
    -h|--help) usage; exit 0 ;;
    rust|features|render|all) ;;
    *) usage >&2; exit 64 ;;
esac

# Golden tests treat even UPDATE_GOLDEN=0 as acceptance. Verification must never
# inherit that opt-in and quietly rewrite its expected output.
if [[ ${UPDATE_GOLDEN+x} ]]; then
    echo 'error: unset UPDATE_GOLDEN before verification; accept reviewed golden changes separately.' >&2
    exit 64
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

run() {
    if [[ ${GITHUB_ACTIONS:-} == true ]]; then
        printf '::group::%s\n' "$*"
    else
        printf 'Running: %s\n' "$*"
    fi
    local status=0
    "$@" || status=$?
    if [[ ${GITHUB_ACTIONS:-} == true ]]; then
        echo '::endgroup::'
    fi
    if [[ $status -ne 0 ]]; then
        printf 'Check failed (%s): %s\n' "$status" "$*" >&2
        echo 'Render mismatch images, when produced, are in target/golden-actual/.' >&2
        exit "$status"
    fi
}

if [[ $mode == rust || $mode == all ]]; then
    run cargo fmt --all -- --check
    run cargo clippy --locked --all-targets -- -D warnings
    run cargo test --locked
fi

if [[ $mode == features || $mode == all ]]; then
    run cargo test --locked --lib --no-default-features
    run cargo test --locked --lib --no-default-features --features ble
    run cargo test --locked --lib --no-default-features --features serial
    run cargo build --locked --bin thermark --no-default-features --features ble
    run cargo build --locked --bin thermark --no-default-features --features serial
fi

if [[ $mode == render ]]; then
    run cargo test --locked --test golden --test fixtures_readme --test label_placement
fi
