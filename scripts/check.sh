#!/usr/bin/env bash
# Shared local/CI validation. No printer access or saved-config changes.
set -euo pipefail

usage() {
    cat <<'HELP'
Usage: scripts/check.sh [rust|features|render|all]

  rust      Formatting, Clippy, and the default-feature tests and docs (default)
  features  Clippy, unit/CLI tests, and docs for each transport feature set
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

# Golden acceptance is an explicit, separate operation. Verification must never
# inherit UPDATE_GOLDEN=1 and quietly rewrite its expected output.
if [[ ${UPDATE_GOLDEN+x} ]]; then
    echo 'error: unset UPDATE_GOLDEN before verification; accept reviewed golden changes separately.' >&2
    exit 64
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# CI manages its own cache lifetime. Keep local build artifacts bounded.
if [[ ${CI:-} != true && ${GITHUB_ACTIONS:-} != true && -z ${CARGO_TARGET_DIR:-} ]]; then
    bash scripts/prune-build-cache.sh
fi

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
    run cargo doc --locked --no-deps
fi

if [[ $mode == features || $mode == all ]]; then
    for feature in none ble serial; do
        feature_args=(--no-default-features)
        if [[ $feature != none ]]; then
            feature_args+=(--features "$feature")
        fi
        run cargo clippy --locked --all-targets "${feature_args[@]}" -- -D warnings
        run cargo test --locked --lib --bins --test cli --test packet_stream "${feature_args[@]}"
        run cargo doc --locked --no-deps "${feature_args[@]}"
    done
fi

if [[ $mode == render ]]; then
    run cargo test --locked --test golden --test fixtures_readme --test label_placement
fi
