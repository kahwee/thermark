# Rust CI on stash.local

Thermark and Denki each have a repository-scoped Docker runner and separate state
volume. The shared image preinstalls Rust, Clippy, rustfmt and Linux build libraries.
Each container has two CPU cores, 2 GB memory and 3 GB memory plus swap, with no
Docker socket, host home, device mounts or published ports. Cargo uses two build jobs.

Run `docker compose up -d --build` in this directory on the NAS. Register each
container using a short-lived token from its repository's Actions runners API:

```sh
gh api --method POST repos/kahwee/thermark/actions/runners/registration-token --jq .token |
  ssh stash.local 'docker exec -i thermark-gh-runner bash -c '\''
    read -r ACTIONS_RUNNER_INPUT_TOKEN
    export ACTIONS_RUNNER_INPUT_TOKEN
    ./config.sh --unattended --url https://github.com/kahwee/thermark \
      --name stash-thermark-ci --labels thermark-ci --work _work
  '\'''
```

Substitute `denki` for `thermark` to register Denki. Runner auto-updates remain enabled.
Deployment directory on the NAS: `~/rust-github-runner`.

Only main pushes and manual main runs use these runners. PRs use GitHub-hosted
Linux, and Denki's macOS build remains hosted. Repository settings require approval
for all external fork contributors. The root-owned job-start hook also rejects
other events and refs before checkout. Do not approve an external PR workflow that
requests these runners. Containers are not a security boundary against hostile code.

Cargo registry/toolchains persist under `/home/runner/.cargo` and `.rustup`.
Build artifacts persist under `/home/runner/.cache/thermark-target` or `denki-target`,
outside checkout cleanup. Cargo still checks dependency, source, compiler and feature
fingerprints; tests execute on every run. Hosted jobs retain their existing GitHub
cache. Denki retains its pinned audit binary but refreshes the advisory database on
every audit. No remote artifact cache upload/download is needed on the NAS.

Inspect with `docker compose ps` and `docker compose logs --tail=100`. Rebuild the
image for OS/library updates. Workflow toolchain setup updates stable Rust. Clear
only the affected `.cache/*-target` directory while that runner is idle if reclaiming
disk space or investigating a cache issue; do not remove registration volumes.

Validate workflow edits with actionlint and dispatch the main workflows. Compare
step durations across a cold run and a rerun of the same commit, including checkout
and queue costs. The NAS does not replace macOS or physical printer testing.
