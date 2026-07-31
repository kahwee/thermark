//! Library error type and printer `In_PrintError` (0xDB) reason codes.

use crate::packet::PacketError;
use std::fmt;

/// Public result alias for the library.
pub type Result<T> = std::result::Result<T, Error>;

/// Typed errors for library callers (`match` / inspect without string scraping).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Printer returned `In_PrintError` (0xDB).
    #[error("printer error: {0}")]
    Printer(PrinterFault),

    /// Timed out waiting for a response packet.
    #[error(
        "timeout waiting for response {expected:#04x} after request {request:#04x} \
         (tip: printer offline/busy, or vendor app holding BLE — try `thermark doctor --use-config`)"
    )]
    Timeout { expected: u8, request: u8 },

    /// Density outside 1..=5.
    #[error("invalid density {0} (need 1..=5)")]
    InvalidDensity(u8),

    /// Image wider than printhead.
    #[error(
        "image width {width}px exceeds printer max {max}px \
         (tip: pass --label 50x30 so thermark scales to the sticker canvas)"
    )]
    ImageTooWide { width: u32, max: u32 },

    /// Unsupported rotation angle.
    #[error("rotate must be 0/90/180/270, got {0}")]
    InvalidRotation(u32),

    /// Raster dimensions exceed what SetPageSize can express (u16) or the head.
    #[error("image size {width}x{height}px is out of range for this printer")]
    ImageTooLarge { width: u32, height: u32 },

    /// Job streamed but the printer never confirmed PrintEnd.
    #[error(
        "print job finished without printer confirmation (end_print never succeeded; \
         label may still have printed — check the device)"
    )]
    PrintNotConfirmed,

    /// Printer replied to a setup command but did not accept it (ACK payload was 0).
    #[error("printer rejected {step} (cmd {cmd:#04x})")]
    CommandRejected {
        /// Human-readable step name (`set_density`, `start_print`, …).
        step: &'static str,
        /// Request command byte that was rejected.
        cmd: u8,
    },

    /// Bad `--label` / size string (e.g. not `50x30`).
    #[error("invalid label size: {0}")]
    InvalidLabel(String),

    /// Packet framing / checksum.
    #[error(transparent)]
    Packet(#[from] PacketError),

    /// BLE / serial / I/O transport failure.
    #[error("transport: {0}")]
    Transport(String),

    /// Font load / parse failure.
    #[error("font: {0}")]
    Font(String),

    /// QR encode / layout failure.
    #[error("qr: {0}")]
    Qr(String),

    /// Image codec / open failure.
    #[error("image: {0}")]
    Image(#[from] image::ImageError),

    /// Filesystem I/O.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Generic message (layout, parse, etc.).
    #[error("{0}")]
    Message(String),
}

impl Error {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }

    pub fn transport(s: impl Into<String>) -> Self {
        Self::Transport(s.into())
    }

    pub fn font(s: impl Into<String>) -> Self {
        Self::Font(s.into())
    }

    pub fn qr(s: impl Into<String>) -> Self {
        Self::Qr(s.into())
    }

    pub fn invalid_label(s: impl Into<String>) -> Self {
        Self::InvalidLabel(s.into())
    }
}

/// A fault code reported by the printer in an `In_PrintError` (`0xDB`) reply,
/// or in the 10-byte form of a `PrintStatus` reply.
///
/// Deliberately a thin newtype over the byte rather than an enum of every code
/// the firmware can emit. thermark branches on three of them; the rest exist to
/// be *reported*, and an enum with forty variants bought nothing for that but a
/// large table to keep correct. An unrecognised byte round-trips unchanged
/// instead of collapsing into an `Unknown` variant that loses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrinterFault(pub u8);

impl PrinterFault {
    /// Lid is open. Blocks printing until closed.
    pub const COVER_OPEN: Self = Self(0x01);
    /// No labels loaded.
    pub const NO_PAPER: Self = Self(0x02);
    /// Battery too low to drive the head reliably.
    pub const LOW_BATTERY: Self = Self(0x03);

    pub fn from_u8(code: u8) -> Self {
        Self(code)
    }

    pub fn code(self) -> u8 {
        self.0
    }

    /// Plain-English meaning, where the condition is one this printer family is
    /// known to report. Unrecognised codes say so rather than guess.
    pub fn description(self) -> &'static str {
        match self.0 {
            0x01 => "Cover / lid is open",
            0x02 => "No paper / labels loaded",
            0x03 => "Battery too low to print",
            0x04 => "Battery fault",
            0x05 => "Print cancelled at the printer",
            0x06 => "Invalid print data",
            0x07 => "Print head overheated",
            0x08 => "Paper ran out mid-job",
            0x09 => "Printer is busy",
            0x0a => "Print head not detected",
            0x0b => "Temperature too low to print",
            0x0c => "Print head loose / not seated",
            0x0d => "No ribbon installed",
            0x0e => "Wrong ribbon type",
            0x0f => "Ribbon exhausted",
            0x10 => "Wrong paper / label type",
            0x11 => "Printer rejected the paper settings",
            0x12 => "Printer rejected the print mode",
            0x13 => "Printer rejected the density setting",
            0x14 => "Failed to write the label's RFID tag",
            0x15 => "Printer rejected the margin setting",
            0x16 => "Communication error",
            0x17 => "Disconnected during print",
            0x18 => "Canvas / image parameter out of range",
            0x19 => "Invalid rotation parameter",
            0x1a => "Malformed parameter",
            0x1b => "Abnormal paper output",
            0x1c => "Paper check failed",
            0x1d => "RFID tag not written",
            0x1e => "Density setting not supported",
            0x1f => "Print mode not supported",
            0x20 => "Label material setting rejected",
            0x21 => "Label material not supported",
            0x22 => "Printer cannot write RFID tags",
            0x32 => "Illegal page / job parameter",
            0x33 => "Illegal ribbon page parameter",
            0x34 => "Printer timed out waiting for data",
            0x35 => "Unrecognised ribbon",
            _ => "Unrecognised printer fault",
        }
    }

    /// What the user can actually do about it.
    pub fn hint(self) -> Option<&'static str> {
        Some(match self.0 {
            0x01 => "Close the B1 cover fully until it clicks.",
            0x02 | 0x08 | 0x1c => {
                "Load a label roll with 2\u{2013}5 mm sticking out of the exit slot."
            }
            0x03 | 0x04 => "Charge the printer, then try again.",
            0x07 => "Wait for the print head to cool, then retry.",
            0x09 => "Wait for the current job to finish, or power-cycle the printer.",
            0x10 | 0x11 | 0x20 => {
                "Use compatible labels for this model; check the label type setting."
            }
            0x14 | 0x1d | 0x22 => {
                "RFID consumable issue \u{2014} try official labels or a different roll."
            }
            0x06 | 0x18 | 0x32 => {
                "Check image size (B1 max width 384 px) and print-task parameters."
            }
            _ => return None,
        })
    }
}

impl fmt::Display for PrinterFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02X} — {}", self.0, self.description())?;
        if let Some(h) = self.hint() {
            write!(f, " ({h})")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_we_saw() {
        let e1 = PrinterFault::from_u8(0x01);
        assert_eq!(e1, PrinterFault::COVER_OPEN);
        assert!(e1.description().contains("open"));

        let e2 = PrinterFault::from_u8(0x02);
        assert_eq!(e2, PrinterFault::NO_PAPER);
        assert!(e2.description().to_lowercase().contains("paper"));
    }

    #[test]
    fn from_u8_roundtrip_known_and_unknown() {
        for code in [0x01u8, 0x02, 0x14, 0x35, 0x99] {
            let e = PrinterFault::from_u8(code);
            assert_eq!(e.code(), code);
            let _ = e.to_string();
            let _ = e.hint();
        }
    }

    #[test]
    fn typed_error_is_matchable() {
        let err = Error::Printer(PrinterFault::NO_PAPER);
        match err {
            Error::Printer(PrinterFault::NO_PAPER) => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn packet_error_converts() {
        let err: Error = PacketError::BadHead.into();
        assert!(matches!(err, Error::Packet(_)));
    }
}
