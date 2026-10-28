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

Version 0.36.1 is available on GitHub, Homebrew and crates.io from immutable
`v0.36.1` at `cb4eb488e89b98afa63b4fd1b7cb8cda0bd83331`.
[docs.rs build 5044423](https://docs.rs/crate/thermark/0.36.1/builds/5044423) succeeded.
Later main-branch maintenance remains Unreleased. Its registry upload used existing
Cargo authentication; no tag-triggered OIDC upload has been exercised.

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

## Start a future release

After the exact-source checks above pass, push one new version tag. That starts
the independent GitHub binary and crates.io workflows; Homebrew remains a
separate update after binary verification. Registry publishing requires the
matching crates.io trusted-publisher binding described below.

Set the intended version and tag the tested commit:

```sh
release_version='<new-version>'
git tag -a "v$release_version" -m "thermark $release_version"
git push origin "v$release_version"
```

Never reuse or move an existing tag. Tags pushed before `publish-crate.yml` was
activated do not acquire a publisher run retroactively. Reconcile the current
manifest, lockfile, tags and distribution status before choosing a future
version; activation does not backfill older versions.

## Verify GitHub binaries

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

## Verify crates.io publication

The active [Publish crate workflow](../.github/workflows/publish-crate.yml) runs
only on future `v*` tag pushes from user accounts. It rejects unstable versions or
a tag/manifest/root-lock mismatch, requires the exact source to be on main with
successful push CI, validates the package inventory, compiles the packaged source
with `cargo publish --locked --dry-run`, and audits locked dependencies before
authentication. It then uses the official crates.io action to obtain a short-lived
OIDC credential and runs one locked publish. Branch pushes and manual dispatches
do not trigger this workflow.

The publishing job has only `contents: read`, `actions: read`, and `id-token: write`.
It uses no GitHub environment or long-lived Cargo secret. The separate crates.io
trusted-publisher binding must match owner `kahwee`, repository `thermark`,
workflow `publish-crate.yml`, and a blank environment. GitHub access alone does
not establish that registry trust; confirm the binding before a future release.

The matching binding was configured and read back on 2026-10-28. This confirms
configuration, not a completed OIDC upload.

Wait for that tag's Publish crate run, then confirm the version is visible on
crates.io and that docs.rs builds its API documentation. Check a clean
`cargo install thermark --locked --version "$release_version"` when practical.
If registry authentication or publication fails, report crates.io as pending
even when GitHub binaries and Homebrew are available. Reconcile registry state
before retrying: a successful upload can precede a later verification failure.

## Publishing automation and recovery

`scripts/release-assets.py` compares existing asset hashes before uploading
missing files; differing published bytes stop recovery. Retain original archives
and tags. Rebuild timestamps can change bytes without a source change.

The tag-only crates.io workflow is active, but activation itself does not publish
anything or prove the external trust binding. Existing published versions need
source/checksum reconciliation. Historical tags without this workflow and its
guards need a separately reviewed recovery procedure; never move or repush them
to trigger publication.

The source workflow token cannot write the Homebrew tap. Automatic tap updates
need separately approved credentials scoped to that repository. This workflow
does not configure registry trust or grant cross-repository publishing access.
