#!/usr/bin/env bash
# Keep full streaming logs and expose concise failures through the checks API.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
log="$(mktemp)"
trap 'rm -f "$log"' EXIT
if scripts/check.sh all 2>&1 | tee "$log"; then
    exit 0
fi
python3 - "$log" <<'PY'
from pathlib import Path
import sys
text = '\n'.join(Path(sys.argv[1]).read_text(errors='replace').splitlines()[-90:])[-24000:]
text = text.replace('%', '%25').replace('\r', '%0D').replace('\n', '%0A')
print('::error title=Rust validation failed::' + text)
PY
exit 1
