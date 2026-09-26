# Contributing to thermark

The most useful contribution is a reproducible report from a printer someone
actually owns. B1 over BLE is the only hardware-tested path today.

## Test another printer

1. Follow the [installation guide](docs/installation.md) for Homebrew, Cargo, or release downloads.
2. Run `thermark --version`, `thermark scan`, and `thermark doctor --json`.
3. Choose the correct model and actual label dimensions. Experimental paths
   require `--allow-experimental`; review `thermark tasks` before printing.
4. Print public demo content, check its QR with a phone, and repeat once. Note
   missing rows, alignment, density, and whether the printer feeds correctly.
5. Open a [compatibility report](https://github.com/kahwee/thermark/issues/new?template=hardware-report.yml).

Use fake Wi-Fi credentials and example.com URLs. Review screenshots and logs;
never publish device addresses, serial numbers, RFID IDs, or actual passwords.
`doctor --json` is designed for public reports. Raw debug logs and
`identify --json` are not privacy-safe replacements.

## Code and documentation

Read [AGENTS.md](AGENTS.md) for module ownership and product boundaries.
Run `scripts/check.sh all` for dependency, transport, or shared API changes;
`scripts/check.sh` is sufficient for other Rust changes. Rendering baselines
must be visually reviewed before acceptance. Tests and mock transports do not
establish hardware verification.

Small fixes, clearer setup instructions, reproducible bugs, and reports of
accessibility or installation problems are welcome. Keep personal data in
ignored `local/`. Mention the OS, thermark version, and a minimal example.
