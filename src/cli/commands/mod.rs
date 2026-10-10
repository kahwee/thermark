//! One module per command group. Each is a thin adapter: parse-time values in,
//! library calls out, human-readable output to stdout.

pub mod batch;
pub mod config;
pub mod device;
pub mod doctor;
pub mod print;
pub mod setup;
pub mod sticker;
