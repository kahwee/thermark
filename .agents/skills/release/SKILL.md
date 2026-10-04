---
name: release
description: Prepare, publish, resume, or verify a Thermark release, including version selection, Rust and package checks, GitHub binaries, archive checksums, Homebrew updates, and crates.io publication. Use when the user asks to release Thermark or review its release readiness.
---

# Release Thermark

Carry the requested release through publication and read-back verification.
Track GitHub binaries, Homebrew, and crates.io separately; report the actual
status of each. A package dry run is not registry publication.

## Scope and prerequisites

- The default repositories are `kahwee/thermark` and its distribution tap,
  `kahwee/homebrew-thermark`. Verify access and repository identity before writes.
- Use the user's requested scope. A request to release authorizes the release
  steps in that scope; do not add another confirmation round. A request to
  prepare, review, verify, or create this skill does not authorize publication.
  Reuse authorization already given in the conversation.
- Use connected GitHub tools or an authenticated `gh` CLI, Git, Python 3.12+,
  Cargo, and the toolchain/native libraries required by the current source.
  The bundled verifier requires `gh` and `curl`. This plugin adds instructions
  and a local script; it does not grant GitHub or registry access.
- Never print tokens, dump the environment, inspect credential contents, or
  persist credentials in source. Inspect readiness, credential names, and file
  existence safely. Use the environment's runtime guidance when applicable.

## Inspect and reconcile

1. Find or clone the checkout. Read `AGENTS.md`, `docs/releasing.md`, relevant
   hardware notes, `Cargo.toml`, `rust-toolchain.toml`, and the Rust, audit, and
   release workflows. Current repository instructions own the process.
2. Inspect the working tree and preserve unrelated changes. Fetch the source
   branch and tags. Inspect published releases, the tap formula, crates.io's
   available versions, and CI runs for the exact source commit.
3. Determine whether this is a new release or an interrupted release. Reconcile
   existing tags, workflow runs, release assets, and distribution versions
   before writing. Reuse observed run/release IDs; do not duplicate a successful
   or uncertain operation. Never move a published tag.
4. Choose the version from changes since the latest release, honoring an
   explicit user version. For this 0.x crate, public API changes require a
   minor bump. Do not hardcode the latest version or the toolchain from an
   earlier session. Keep a concise plan and communicate progress during waits.

## Prepare and check the exact source

1. Update package/lockfile versions, dated changelog entries and comparison
   links, installation examples, and minimum-Rust documentation. Keep the
   manifest, toolchain file, and workflows compatible. Review migration notes.
2. Run the repository's required checks. At the current layout, shared API,
   dependency, or transport changes require `scripts/check.sh all`. Unset
   `UPDATE_GOLDEN`; inspect mismatches instead of accepting them to pass.
3. Inspect `cargo package --locked --list` for required licenses, tests, and
   `local/README.md`; exclude personal artwork, identities, Wi-Fi credentials,
   runner infrastructure, and build artifacts. `--allow-dirty` is acceptable
   for a provisional inventory, but final package verification uses the
   committed source. Run `cargo publish --locked --dry-run` on that source.
4. Commit only authorized changes and publish the exact checked commit.
   Require the Linux/macOS Rust workflow and dependency audit to pass on that
   SHA before tagging. Resolve failures from their logs. See
   [recovery guidance](references/recovery.md) for authentication and CI issues.

## Publish GitHub binaries

1. Confirm the intended version tag does not already point elsewhere, package
   versions match, and required CI passed on the exact SHA. Create an annotated
   `v<version>` tag and publish it. Verify the remote tag resolves to that SHA.
2. Monitor the tag-triggered Release binaries workflow through its publish
   job. A manual workflow run only builds artifacts in the current workflow.
   Preserve every required platform and feature variant.
3. Read the published release and assets. The current matrix emits eight
   archives: Linux/macOS × ARM64/X64 × BLE/full, plus eight checksum files.
   Check the live workflow before assuming this matrix in a future release.
4. Run the bundled verifier against the published version and expected SHA:

   ```sh
   python3 <skill-directory>/scripts/verify_release.py \
     --tag v<version> --expected-commit <tested-sha> \
     --output <temporary-download-directory> --smoke \
     --font <checkout>/tests/fonts/DejaVuSans.ttf
   ```

   The script only reads remote state and downloads/runs published binaries.
   If the skill is provided as cloud resources, read the script through the
   skill provider and materialize it in the execution workspace first; do not
   treat a `skill://` URI as a filesystem path.
   Review that the source and release are trusted before executing binaries.
   It validates asset names, sizes, digests, sidecar checksums, archive members,
   and an offline label from the host's BLE binary. Keep temporary config and
   unset `THERMARK_ADDR` for child commands. Confirm no config was created.
5. Publish concrete notes: install commands, changed behavior, API migration,
   minimum Rust, and actual distribution availability. Only B1+b1 over BLE is
   hardware-verified; tests and previews do not establish new hardware support.

## Align distributions

- **Homebrew:** update only the version, macOS ARM64/X64 BLE URLs, and hashes
  in `Formula/thermark.rb`, using the verified published archives. Review and
  publish the tap change, then read it back. Run install/upgrade and
  `brew test thermark` on macOS when available. Distinguish a formula update
  from a macOS installation test.
- **crates.io:** with authorized registry access, run `cargo publish --locked`
  once and verify the version is visible. Check docs.rs builds the version;
  verify a clean Cargo install when practical. If access is missing, complete
  the independent authorized steps and mark registry publication as pending.
  Do not ask for a token in chat or silently claim all destinations are done.
- Reconcile the state before retrying any write. Source fixes after a published
  release need a new version. For an unpublished tag, diagnose before choosing
  a recovery; do not automatically rewrite tags or clobber published assets.

## Handoff

Return the release link, version/SHA, concise checks, distribution status, and
material limits or blockers. Link `docs/releasing.md` for the repository's
checklist. Keep working until the authorized steps finish or an actual access
or infrastructure blocker prevents them. Do not end with an offer to continue.
