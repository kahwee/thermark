//! High-level printer client and parsed status types.

mod client;
mod info;

pub use client::{OnTimeout, Pacing, PrintOptions, PrinterClient, compose_for_label};
pub use info::{
    BATTERY_MAX, Heartbeat, InfoValue, PrinterSummary, RfidInfo, describe_battery,
    describe_battery_str,
};
