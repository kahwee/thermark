//! # thermark
//!
//! Local **sticker printing** for pocket thermal label printers (B1, B21, D11, …)
//! over Bluetooth LE or USB serial — no vendor app, no cloud.
//!
//! Typical jobs: **guest Wi‑Fi join stickers**, QR + text links, inventory tags,
//! name badges, line-art stickers, and calibration patterns.
//!
//! ## Features
//!
//! | Feature | Default | Enables |
//! |---------|---------|---------|
//! | `ble` | yes | [`BleTransport`] via btleplug |
//! | `serial` | yes | [`SerialTransport`] via serialport |
//!
//! Protocol-only / mock testing: `cargo test --no-default-features`.
//!
//! Protocol: see `protocol.rs` for command IDs and `packet.rs` for framing.

pub mod config;
pub mod doctor;
pub mod errors;
pub mod font;
pub mod geometry;
pub mod image_encode;
pub mod label;
pub mod mock;
pub mod packet;
pub mod print_task;
pub mod printer;
pub mod protocol;
pub mod transport;
pub mod types;
pub mod wifi;

pub use config::{Config, ConnPref};
pub use errors::{Error, PrinterFault, Result};
pub use geometry::{DEFAULT_B1_LABEL, HEAD_NARROW_PX, HEAD_WIDE_PX, LabelMm, LabelPx, PX_PER_MM};
pub use image_encode::Raster;
pub use label::{TextAlign, TextLabelOptions, TextSide, make_qr_label_opts, make_text_label};
pub use mock::MockTransport;
pub use packet::Packet;
pub use print_task::{PrintTask, SupportStatus, effective_max_width_px, hardware_matrix};
pub use printer::{
    Heartbeat, InfoValue, OnTimeout, Pacing, PrintOptions, PrinterClient, PrinterSummary, RfidInfo,
};
pub use protocol::Model;
pub use transport::{
    BleCandidate, BleMatchMode, Transport, name_looks_like_label_printer, score_ble_candidate,
    select_ble_candidate,
};
pub use types::{Density, Rotation, Threshold};
pub use wifi::{WifiLabelOptions, WifiSecurity, make_wifi_label, wifi_qr_payload, wifi_side_text};

#[cfg(feature = "ble")]
pub use transport::{BleTransport, PRINTER_CHAR, PRINTER_SERVICE};

#[cfg(feature = "serial")]
pub use transport::SerialTransport;
