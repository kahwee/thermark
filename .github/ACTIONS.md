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

- [rust.yml](workflows/rust.yml), job `build`: `dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec`; `Install system dependencies`.

Steps after a group wait for it to finish. Keep prerequisites before the group and dependent build, package, or deployment work afterward. Do not run commands that overwrite the same build directory or coverage output together. Parallel steps share the job workspace; they do not provide separate machines.

## Action versions

Verified against upstream stable releases on 2026-10-28. Every external action is
pinned to the resolved commit SHA; comments name the release. The Rust toolchain
action has no numbered releases, so its pin follows the upstream `stable` branch
as of that date. Its explicit `toolchain` input still selects Rust 1.99.0.

### Active workflows and composite actions

| Action | Version | Commit |
| --- | --- | --- |
| [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache) | [v2.9.3](https://github.com/Swatinem/rust-cache/releases/tag/v2.9.3) | `d1527c8f67ef9af357290475d4af4b0fab8f12e0` |
| [actions/checkout](https://github.com/actions/checkout) | [v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1) | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| [actions/download-artifact](https://github.com/actions/download-artifact) | [v8.0.2](https://github.com/actions/download-artifact/releases/tag/v8.0.2) | `9000827ccba6bdab643e8b6fd33ac0654aef8333` |
| [actions/setup-python](https://github.com/actions/setup-python) | [v7.1.0](https://github.com/actions/setup-python/releases/tag/v7.1.0) | `6d4fd22b20465ccc0f7354476588e8df7c9ca0e0` |
| [actions/upload-artifact](https://github.com/actions/upload-artifact) | [v7.0.2](https://github.com/actions/upload-artifact/releases/tag/v7.0.2) | `cf430e030ddbb5b0abf93d22962f4752f3646cd9` |
| [dtolnay/rust-toolchain](https://github.com/dtolnay/rust-toolchain) | [stable](https://github.com/dtolnay/rust-toolchain/commit/b725eda525362e7cf34af9a7243cac1e3f01a7ec) | `b725eda525362e7cf34af9a7243cac1e3f01a7ec` |

### Inactive release proposals

The YAML files in `release-proposals/` are outside `.github/workflows` and do not
run. Their shared action references match the table above. The additional action is:

| Action | Version | Commit |
| --- | --- | --- |
| [rust-lang/crates-io-auth-action](https://github.com/rust-lang/crates-io-auth-action) | [v1.0.6](https://github.com/rust-lang/crates-io-auth-action/releases/tag/v1.0.6) | `3268eb413133e2960fa4ae0e963ca76380c54cd9` |

Updating these pins does not activate a proposal or configure registry trust.
The active JavaScript actions use Node.js 24; keep runners compatible with each
upstream action's runner requirements. The checked-in jobs use GitHub-hosted
runners.

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

Linux CI also checks the Cargo package inventory and runs a locked publish dry run.
This catches missing bundled files and packaged-source compilation failures before
release tagging. It uses the existing read-only permissions, without registry
credentials. The crate publisher remains an inactive proposal outside workflows.
