# Release thermark

A release has three distribution steps: GitHub binaries, the Homebrew tap,
and crates.io. Finish and verify each step before reporting it as available.
Only B1 with the b1 task over BLE is hardware-verified; automated checks do not
establish support for another printer or transport.

The repository's [release skill](../.agents/skills/release/SKILL.md) guides
preparation, publication, and recovery. Its verifier checks an existing release
without publishing or editing remote state:

```sh
python3 .agents/skills/release/scripts/verify_release.py \
  --tag v<version> --expected-commit <tested-sha> \
  --output <temporary-download-directory> --smoke \
  --font tests/fonts/DejaVuSans.ttf
```

## Prepare the source

1. Review changes since the latest release and choose the version. While the
   crate is 0.x, use a minor bump for public API changes.
2. Update `Cargo.toml` and the thermark entry in `Cargo.lock`. Move pending
   changelog entries into a dated release section, keep an empty Unreleased
   section, and update comparison links. Align installation examples and the
   documented minimum Rust version with the new release. Run
   `python3 scripts/render-docs.py` to refresh the build/image manifest.
3. Keep `Cargo.toml`'s `rust-version`, `rust-toolchain.toml`, and the CI and
   release workflow toolchains compatible.
4. Run `scripts/check.sh all`. Review render mismatches without accepting new
   goldens just to pass. Run `cargo package --locked` and check that the
   package includes licenses, tests, and `local/README.md`, with no personal
   artwork, printer identifiers, or real Wi-Fi labels.
   Run `python3 scripts/check-package.py` to validate Cargo's actual inventory,
   then `cargo publish --locked --dry-run` to compile the packaged source.
   Linux CI runs these checks on each main push and pull request without registry
   authentication or upload. The metadata guard also requires tag, manifest and
   root lockfile versions to agree.
5. Commit and push the source. Wait for the Rust workflow on that exact commit
   to pass on Linux and macOS, and for the dependency audit to pass. Resolve
   failures before tagging.

## Publish GitHub binaries

Set the intended version and tag the tested commit:

```sh
release_version='<new-version>'
git tag -a "v$release_version" -m "thermark $release_version"
git push origin "v$release_version"
```

`.github/workflows/release.yml` checks that the tag matches the package version,
tests full and BLE-only builds, and packages both variants for Linux and macOS,
on ARM64 and X64. Publication waits for every platform. A manual workflow run
builds artifacts but does not publish a release.

1. Wait for the tag's Release binaries workflow to pass, including its publish
   job. Expect eight `.tar.gz` archives and eight matching `.sha256` files.
2. Download the published assets and verify every checksum. Inspect archive
   contents: each contains `thermark`, `README.md`, `LICENSE`, and `LICENSE-FONTS`.
3. Extract the host's archive and run its installed binary's `--version`,
   `--help`, and `tasks`. Render the first-label QR offline with a temporary
   `THERMARK_CONFIG`, `THERMARK_ADDR` unset, and a public demo URL. Confirm a
   PNG is created and no printer configuration is saved.
4. Write release notes with installation instructions, the concrete changes,
   public API migration notes, and the existing hardware-support limits.

If a build fails, fix the failure and rerun the failed jobs for the same tagged
source. Do not move an already published tag. A source fix after publication
requires a new version.

## Update Homebrew

In `kahwee/homebrew-thermark`, update `Formula/thermark.rb` with the version,
macOS ARM64/X64 BLE archive URLs, and SHA-256 hashes from the verified release
assets. Keep both architectures aligned. Review the diff and push the tap.

On macOS, run `brew update`, `brew upgrade kahwee/thermark/thermark` (or install
it), and `brew test thermark`. Verify `thermark --version`. Updating the formula
alone is not a local macOS installation test.

## Publish crates.io

Use an authorized Cargo registry credential; never put it in source or logs.
`cargo publish` needs separate access from GitHub and Homebrew.

```sh
cargo publish --locked --dry-run
cargo publish --locked
```

Confirm the new version is visible on crates.io and that docs.rs builds its
API documentation. Check a clean `cargo install thermark --locked --version
"$release_version"` when practical. If registry access is unavailable, report
crates.io as pending even if binaries and Homebrew are published.

## Publishing automation and recovery

`scripts/release-assets.py` compares existing asset hashes before uploading
missing files; differing published bytes stop recovery. Retain original archives
and tags. Rebuild timestamps can change bytes without a source change.

The [crates.io publisher proposal](../.github/release-proposals/publish-crate.yml)
is inactive outside `.github/workflows`. Activation needs approval for
`contents: read`, `actions: read`, `id-token: write` and a trusted publisher:
owner `kahwee`, repository `thermark`, workflow `publish-crate.yml`, blank
environment. It checks human-pushed stable tags, exact source CI and packaging
before temporary authentication. Existing versions require source/checksum
reconciliation; historical tags without guards need separate recovery review.

The source workflow token cannot write the Homebrew tap. Automatic tap updates
need separately approved credentials scoped to that repository. No registry trust
or cross-repository publishing access is configured by the proposal.
