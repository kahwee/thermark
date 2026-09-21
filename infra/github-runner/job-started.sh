#!/usr/bin/env bash
set -euo pipefail
# This root-owned hook runs before checkout, outside the writable work tree.
case "${GITHUB_REPOSITORY:-}:${GITHUB_REF:-}:${GITHUB_EVENT_NAME:-}" in
  kahwee/thermark:refs/heads/main:push|kahwee/thermark:refs/heads/main:workflow_dispatch|kahwee/denki:refs/heads/main:push|kahwee/denki:refs/heads/main:workflow_dispatch) ;;
  *) echo 'This runner only accepts trusted main pushes and manual main runs.' >&2; exit 1 ;;
esac
