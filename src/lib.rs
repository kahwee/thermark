//! # thermark
//!
//! Talk to pocket thermal label printers (B1, B21, D11, …) over Bluetooth LE or USB
//! serial without the official app.
//!
//! Protocol reference: the protocol notes in src/protocol.rs

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

pub use errors::{Error, PrinterErrorCode, Result};
pub use geometry::{LabelMm, LabelPx, DEFAULT_B1_LABEL, PX_PER_MM};
pub use mock::MockTransport;
pub use packet::Packet;
pub use print_task::{hardware_matrix, PrintTask, SupportStatus};
pub use printer::{PrintOptions, PrinterClient};
pub use protocol::Model;
pub use transport::{BleTransport, SerialTransport, Transport};
