"""Validate Cargo's actual package inventory; no credentials or uploads."""
import subprocess
import sys

REQUIRED = {
    'Cargo.lock',
    'Cargo.toml',
    'Cargo.toml.orig',
    'LICENSE',
    'LICENSE-FONTS',
    'README.md',
    'assets/fonts/DejaVuSans.ttf',
    'local/README.md',
    'src/lib.rs',
    'src/main.rs',
    'tests/fonts/LICENSE-DejaVu.txt',
}
ALLOWED_FILES = REQUIRED | {'local/README.md', 'AGENTS.md', 'LICENSE-FONTS', 'rust-toolchain.toml', 'CHANGELOG.md', 'CONTRIBUTING.md'} | {".cargo_vcs_info.json"}
ALLOWED_DIRS = ('assets/', 'src/', 'benches/', 'examples/', 'tests/', 'fixtures/', 'docs/', 'scripts/')


def validate(files):
    files = set(files)
    missing = REQUIRED - files
    unexpected = {p for p in files if p not in ALLOWED_FILES and not p.startswith(ALLOWED_DIRS)}
    # Cargo's source allowlist must never admit personal output or runner files.
    unexpected |= {p for p in files if p.startswith(("local/", "infra/", ".github/", ".agents/", "target/")) and p != "local/README.md"}
    if missing or unexpected:
        raise ValueError(f"Missing package files: {sorted(missing)}; unexpected package files: {sorted(unexpected)}")


if __name__ == "__main__":
    result = subprocess.run(["cargo", "package", "--locked", "--list"], capture_output=True, text=True)
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    result.check_returncode()
    validate(result.stdout.splitlines())
