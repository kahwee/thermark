# Maintenance notes

## 2026-09-25: installation and regression coverage

Installation is the first user-facing priority. Keep the README's Homebrew,
Cargo, and archive entry points aligned with `installation.md`; examples should
use the installed `thermark` executable. Source-checkout paths belong in
contributor instructions. Linux dependencies, font setup, upgrades, and common
errors are documented in the installation guide.

Packet framing now has one implementation: `Packet::encode` uses the bounded
writer also used by transports, then returns owned bytes. No public API or
protocol behavior changes. Tests inspect wire fields, checksum parity, and
round trips for every legal payload length, plus known wire examples and
reused-buffer failure behavior.

The CLI first-run regression renders the installation example with a vendored
font, checks 384×240 geometry, independently decodes the QR to example.com, and
checks that no printer configuration was created. The child command runs in a
temporary directory with isolated config/address settings. The existing feature
matrix runs this test even when neither transport is compiled.

The existing module boundaries are useful: profile geometry, task behavior,
packet framing, transport I/O, rendering, and CLI presentation already have
separate owners. Keep improving those boundaries where duplication appears;
splitting files by size alone would make navigation harder without establishing
a clearer owner.

## Ongoing checks

This pass completed `scripts/check.sh all`: formatting, Clippy with warnings
denied, tests, and documentation for default, BLE-only, serial-only, and
no-transport builds. `cargo update --dry-run` found zero compatible updates.
These are local software checks, not hardware verification.

- Review the existing weekly Dependabot updates and RustSec audit results.
  Compatible versions still need the locked feature-matrix checks before merge.
- Run `scripts/check.sh all` for shared code, dependencies, and transport changes.
  Do not accept golden changes merely to pass a test.
- The known unmaintained `ttf-parser` advisory remains visible in auditing;
  evaluate an upstream `ab_glyph` update when a replacement is available.
- For releases, separately verify packaging, an installed binary's offline
  preview, release checksums, and Homebrew version alignment. Build/test success
  is not physical printer verification.
- Keep personal data and proof screenshots under ignored `local/`. Preserve
  local work, configuration, and credentials during repository cleanup.
- The r/niimbot posting request was sent; no public subreddit post has been
  made. Wait for moderator approval before posting the B1 testing invitation.

Remaining adoption work is tracked in [the launch plan](launch-plan.md).
