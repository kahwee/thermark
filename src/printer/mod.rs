//! High-level printer client and parsed status types.

mod client;
mod info;
mod job;
mod pacing;
mod print;
mod query;
mod raw;

pub use client::PrinterClient;
pub use info::{
    BATTERY_MAX, Heartbeat, InfoValue, PrintStatus, PrinterSummary, RfidInfo, describe_battery,
    describe_battery_str,
};
pub use job::{PrintOptions, compose_for_label};
pub use pacing::Pacing;
pub use raw::{OnTimeout, RawPrinter};
