# Local security boundaries

Thermark handles untrusted radio metadata, local media, and sometimes private
label contents. These protections limit accidental disclosure and resource abuse;
they do not make the printer protocol cryptographically authenticated.

## Selecting a printer

`thermark scan --save` saves `id:<platform ID>`, not an advertising name. Saving
requires one unambiguous device; with several devices, select the exact unique
name or ID using `--name`. An unmatched or ambiguous filter fails rather than
falling back to signal strength. Guided `setup` also saves a platform ID after
identification and readiness checks.

Saved defaults from older releases need reenrollment with `scan --save` or
`setup`. They are not silently reused as names. A pinned selector only matches
the exact platform ID, even with `--fuzzy`; another device advertising that ID
as its name does not match. Explicit `--addr`/`THERMARK_ADDR` name selection
remains available and warns that it is unauthenticated.

Enrollment itself is a trust decision: check the discovered device, use a
trusted location, and switch off other printers when identifying yours. GATT
service/characteristic checks and model queries establish compatibility, not
identity. A Bluetooth address/platform ID can be spoofed or change after an OS
reset. These printers offer no verified application-level authentication or
confidentiality guarantee. Avoid sending sensitive labels over BLE in hostile
radio environments; use a trusted local workflow appropriate to the hardware.

Incoming printer notifications are limited to 512 bytes each, with at most 64
queued notifications. Overflow closes the stream and connection and stops
further writes rather than dropping protocol fragments and continuing a job.
A receive returns one fragment; packet decoding handles split responses.
These are application-level bounds, not guarantees about the OS Bluetooth stack.

## Media and output files

Source fonts and compressed images must be regular files of at most 64 MiB.
Reads are bounded on the opened handle, including files that grow after their
metadata is checked. The same nonblocking regular-file checks protect batch
CSV/templates and prepared-job reads, with their existing byte limits; config
files are limited to 1 MiB. Images are checked before pixel decoding: at most 16,384
pixels per side and 256 MiB of decoded pixel storage, with decoder allocation
limits as well. Photos may still exceed the printhead and be scaled normally.
These are per-input budgets, not a cap on total process memory.

Label text is limited to 4,096 UTF-8 bytes before layout, including combining
marks and zero-advance characters. Long ASCII token wrapping measures advances
linearly, and auto-fit does not repeat the same size search. Complex wrapping
also has a cumulative 128 KiB measurement-work budget across the complete
layout, including size retries; pathological input fails with a clear error. Library callers
can use `try_wrap` and `try_fit_size` for explicit limit errors. The infallible
`wrap` preserves input as one unwrapped string when a byte/work budget is exceeded
rather than truncating it.

Image saves use a same-directory private temporary file and atomic replacement.
They reject existing symlinks and special files, never open the destination for
writing, and create Unix output with mode 0600 even when replacing an old public
file. Hard links and a symlink raced into the destination cannot redirect writes
to their targets. Other platforms inherit directory access controls. Use a
trusted private parent directory: path traversal through attacker-controlled
parent directories is not supported as a secure storage boundary. Batch staging
directories are private too. Wi-Fi credentials must not be saved in `fixtures/`.

## Terminal and runner

Human-readable BLE, RFID and diagnostic fields escape terminal controls,
newlines and directional formatting characters. Structured source data remains
unchanged; JSON serialization provides its own escaping.

The self-hosted runner image downloads rustup 1.28.2 from a release-specific URL,
checks repository-pinned SHA-256 digests for x86-64 and ARM64 before execution,
and installs Rust 1.99.1 rather than a moving `stable` toolchain. Rebuilding and
rolling out the image is a separate maintainer action; changing the Dockerfile
does not update an already-running runner.
