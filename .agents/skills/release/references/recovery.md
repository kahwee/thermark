# Release recovery

Always inspect the current repository, remote refs, workflow status, and
published assets before retrying. This guidance records practical recovery
paths, not permission to bypass authentication or repository rules.

## Git push fails but GitHub API access works

Git transport and GitHub API credentials can have different availability.
Diagnose the actual error and current environment/connection readiness. A
configured token placeholder is not proof that access is missing. Preserve
the runtime's proxy and credential setup; do not disable policy or TLS checks.

With authorized GitHub code-write access, an API fallback can publish the
exact tested source:

1. Read the remote branch SHA and require it to equal the local commit's
   parent. Reconcile a changed branch before proceeding.
2. Create blobs/a tree with only the authorized changed paths and the parent
   tree as `base_tree`. Compare the API tree SHA with local `HEAD^{tree}`.
3. Create the commit with the original parent, author, committer, timestamps,
   and exact message bytes. Preserve the message's trailing newline: trimming
   it changes the commit SHA. Compare the API SHA with local `HEAD`.
4. Advance the branch using `force: false`, then read back its SHA. Respect
   required reviews and protected-branch rules; do not weaken them.
5. For an annotated tag, preserve the tag name, target SHA, tagger, timestamp,
   and exact message. Verify the tag object SHA and remote peeled commit before
   treating tag publication as complete.

Use structured JSON or a body file for multiline payloads. Do not interpolate
file contents into shell commands. Uncertain API writes need read-back
reconciliation before another mutation.

## CI failures and resource pressure

Read the failing job logs and identify the failed command. Test assertions,
compiler errors, missing native libraries, advisory failures, and killed
linkers need different remedies. GitHub's connected job-log tool can retrieve
logs when CLI redirect access is unavailable; use existing authorized access.

In the v0.34.0 release, the self-hosted Linux runner's linker was terminated
with signal 9. The runner was configured with a 2 GB memory limit and two Cargo
jobs; a rerun of only the failed job passed using completed build artifacts.
Treat this as evidence of probable resource pressure, not proof for every
future signal-9 failure. No code assertion or advisory was ignored.

For a diagnosed transient failure, rerun failed jobs for the same SHA once and
inspect the result. If it repeats, address the concrete runner/resource issue
or build concurrency within the authorized scope. Do not repeatedly retry,
skip checks, weaken assertions, or deploy runner infrastructure without scope.
Any source/workflow fix changes the release candidate and needs appropriate
checks on its new SHA.

## Partial publication

Keep a status for each destination: source/CI, GitHub binaries, Homebrew,
crates.io, and docs.rs. Record exact SHAs, tags, and observed run/release IDs.
Do not embed tokens, machine-specific paths, or live printer data in records.

- A successful dry run verifies packaging; it does not upload the crate.
- A tag or successful build is not a verified published binary release.
- A tap change is not a local macOS installation test.
- A missing registry credential does not block verified GitHub binaries or an
  authorized Homebrew update. Leave crates.io pending and say so explicitly.
- A published source fix requires a new version; never move an existing
  published tag to disguise a replacement release.
