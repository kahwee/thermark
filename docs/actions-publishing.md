# Actions publishing

`python3 scripts/check-release.py v<version>` verifies the package version, tag
and root Cargo.lock entry without building or publishing.
`python3 scripts/test_check_release.py` covers version mismatch regressions.
Keep the repository release skill and existing required CI as the authority.

## Approval proposal

[The complete crates.io workflow](../.github/release-proposals/publish-crate.yml)
is outside `.github/workflows` and cannot run. Activation requires approval to
move it to `.github/workflows/publish-crate.yml`, grant its publish job
`contents: read`, `actions: read`, `id-token: write`, and bind crate `thermark` to:

- owner `kahwee`, repository `thermark`
- GitHub Actions workflow filename `publish-crate.yml`
- environment blank

Use [crates.io trusted publishing](https://crates.io/docs/trusted-publishing) and
the [official temporary authentication action](https://github.com/rust-lang/crates-io-auth-action).
It checks the human-pushed stable version tag and locked metadata, requires exact
source CI, verifies packaging, then authenticates and publishes. GitHub publication
and registry publication remain separate. No long-lived Cargo credential is added.
Existing versions cannot be overwritten: an already published version requires
read-back source/checksum reconciliation rather than automatically declaring success.

No registry trust or workflow access was enabled by this change. Current local
Cargo credential file existence does not validate publishing authentication.
If the crate does not yet exist, first publication/name ownership is a separate
approved owner action before a trusted-publisher binding can be established.

## Homebrew distribution

The existing binary release is independent of crates.io and the Homebrew tap.
The source workflow's `GITHUB_TOKEN` cannot write `kahwee/homebrew-thermark`.
Automatic tap updates require a separately approved narrow GitHub App/token with
write access to that tap; no such credential was created or installed here.
Until that decision, follow [docs/releasing.md](releasing.md) to update the formula
using verified macOS BLE assets. Do not copy registry tokens into GitHub secrets
or grant a broad cross-repository token to eliminate this approval boundary.

## Immutable GitHub release assets

The existing tag release workflow now uses `scripts/release-assets.py`. It verifies
that the tag resolves to the checked-out source, compares all existing matching
assets by SHA-256 (downloading older assets without server digests), and uploads
only missing files. Differing published bytes stop recovery before any uploads;
there is no `--clobber`. This preserves releases after partial uploads and handles
an identical rerun without replacing assets. Rebuilt archives can differ because
of timestamps: keep the original published artifact and investigate rather than
replacing it. New GitHub releases retain the repository's release notes behavior.
Run `python3 scripts/test_release_assets.py` for isolated recovery regressions.
Existing workflow permissions are unchanged.

Crate publication is triggered directly by human-pushed `v*` tags; no `release: published` cascade from `GITHUB_TOKEN` is assumed. It runs independently of GitHub source/binary publication. Existing immutable historical tags without these new guard files need a separately reviewed recovery procedure; this proposal does not move or republish them.

CI runs the actual metadata CLI against the checked-out manifest/lock and rejects a mismatched tag. The publisher proposal remains unexecuted: packaging dry-run, registry exchange and publication are not proven by guard tests.

Normal PR CI now runs mocked CLI recovery tests for matching assets, missing assets, differing bytes, registry/API 403 errors and a missing (404) release. No real GitHub release write is performed by these fixtures.
