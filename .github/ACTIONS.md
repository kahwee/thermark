# GitHub Actions maintenance

This guide describes the checked-in workflows. Follow each linked YAML file for branch filters, job dependencies, permissions, and release conditions.

## Workflows

| Workflow | Events | Jobs |
| --- | --- | --- |
| [release.yml](workflows/release.yml) | `workflow_dispatch`, `push` | `build`, `publish` |
| [rust.yml](workflows/rust.yml) | `workflow_dispatch`, `push`, `pull_request` | `build` |
| [security.yml](workflows/security.yml) | `workflow_dispatch`, `push`, `pull_request`, `schedule` | `audit` |

## Parallel steps

Independent checks use GitHub Actions [native parallel steps](https://github.blog/changelog/2026-06-25-actions-steps-can-now-be-run-in-parallel/). The following groups run concurrently within a job:

- [rust.yml](workflows/rust.yml), job `build`: `dtolnay/rust-toolchain@686976e191b89faba57d3206551f0f330d8cb249`; `Install system dependencies`.

Steps after a group wait for it to finish. Keep prerequisites before the group and dependent build, package, or deployment work afterward. Do not run commands that overwrite the same build directory or coverage output together. Parallel steps share the job workspace; they do not provide separate machines.

## Action versions

Versions below match the current workflow and composite-action references. SHA-pinned actions remain pinned; compare their commit with the upstream stable release when updating.

| Action | Reference |
| --- | --- |
| [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache) | `6323deb102c322ba6fcbdcafc7e3dddab59af2b6` |
| [actions/checkout](https://github.com/actions/checkout) | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| [actions/download-artifact](https://github.com/actions/download-artifact) | `9000827ccba6bdab643e8b6fd33ac0654aef8333` |
| [actions/setup-python](https://github.com/actions/setup-python) | `5fda3b95a4ea91299a34e894583c3862153e4b97` |
| [actions/upload-artifact](https://github.com/actions/upload-artifact) | `cf430e030ddbb5b0abf93d22962f4752f3646cd9` |
| [dtolnay/rust-toolchain](https://github.com/dtolnay/rust-toolchain) | `686976e191b89faba57d3206551f0f330d8cb249` |

The Rust workflow also runs the release guards and hardware-trial recorder
tests before the Rust feature matrix. Release archives include the bundled
font redistribution terms (`LICENSE-FONTS`). `scripts/check-ci.sh` streams
the same checks and publishes the failure tail as a check annotation when log
storage is unavailable.

## Greenkeeping

[Dependabot configuration](dependabot.yml) checks GitHub Actions weekly and groups their updates. Review the upstream release notes, runtime requirements, permissions, and changes to inputs or artifact behavior before merging. Update SHA pins to the release commit, retaining the version comment where present.

1. Update every reference to the affected action, including local composite actions under `.github/actions/`.
2. Keep frozen dependency installation and the repository’s declared toolchain versions aligned. Cache package downloads with a lockfile-based key; a cache hit does not replace installation or verification.
3. Check workflow YAML and review shell commands. Older workflow linters may not understand native `parallel`; GitHub execution must verify those groups.
4. Run the affected checks and inspect the resulting Actions run. Preserve matrix coverage, build dependencies, artifact paths, and release gates.
5. Refresh this guide when workflows, action references, or parallel groups change.

Use workflow concurrency to cancel superseded check runs where appropriate. Deployment and release cancellation have different consequences: preserve the workflow’s existing policy rather than copying check-run settings blindly. Repository permissions and job-level overrides are defined in the linked YAML files.
