//! # thermark
//!
//! Talk to pocket thermal label printers (B1, B21, D11, …) over Bluetooth LE or USB
//! serial without the official app.
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
//! Protocol reference: the protocol notes in src/protocol.rs

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

pub use config::{Config, ConfigFormat, ConnPref};
pub use errors::{Error, PrinterErrorCode, Result};
pub use geometry::{DEFAULT_B1_LABEL, LabelMm, LabelPx, PX_PER_MM};
pub use mock::MockTransport;
pub use packet::Packet;
pub use print_task::{PrintTask, SupportStatus, hardware_matrix};
pub use printer::{PrintOptions, PrinterClient};
pub use protocol::Model;
pub use transport::Transport;

#[cfg(feature = "ble")]
pub use transport::{BleTransport, PRINTER_CHAR, PRINTER_SERVICE};

#[cfg(feature = "serial")]
pub use transport::SerialTransport;
