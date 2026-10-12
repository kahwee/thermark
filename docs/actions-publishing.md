# Actions publishing

GitHub binaries, Homebrew and crates.io are separate distribution steps.
Use the [release checklist](releasing.md) and [release skill](../.agents/skills/release/SKILL.md)
for publication and recovery.

## Proposed crates.io automation

The [publisher proposal](../.github/release-proposals/publish-crate.yml) is outside
`.github/workflows` and cannot run. Activation requires approval to move it to
`.github/workflows/publish-crate.yml`, grant `contents: read`, `actions: read`
and `id-token: write`, and configure a crates.io trusted publisher for:

- Owner/repository: `kahwee/thermark`
- Workflow: `publish-crate.yml`
- Environment: blank

The proposal checks a human-pushed stable version tag, exact source CI and
packaging before exchanging a temporary registry credential. It does not rely
on a `release: published` cascade from `GITHUB_TOKEN`. Existing versions require
source/checksum reconciliation; they cannot be overwritten. Historical tags
without the guard files need a separate recovery review.

No registry trust or publishing access is configured by the proposal. See
[crates.io trusted publishing](https://crates.io/docs/trusted-publishing).

## Homebrew access

The source workflow's `GITHUB_TOKEN` cannot write `kahwee/homebrew-thermark`.
Update the formula manually using verified macOS BLE assets. Automation needs
a separately approved GitHub App/token scoped to the tap; none is configured.

## Immutable release assets

`scripts/release-assets.py` verifies tag/source identity and compares existing
assets by SHA-256 before uploading missing files. Differing bytes stop recovery;
there is no `--clobber`. Retain published archives even if a rebuild differs
because of timestamps.

CI runs metadata and mocked recovery checks; these do not prove registry access
or perform remote publication. Run the checks locally with:

```sh
python3 scripts/check-release.py v<version>
python3 scripts/test_check_release.py
python3 scripts/test_release_assets.py
```
