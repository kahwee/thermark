//! Library error type and printer `In_PrintError` (0xDB) reason codes.
//!
//! `PrinterErrorCode` source: the protocol reference payloads.ts
//! (<the protocol reference>).

use crate::packet::PacketError;
use std::fmt;

/// Public result alias for the library.
pub type Result<T> = std::result::Result<T, Error>;

/// Typed errors for library callers (`match` / inspect without string scraping).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Printer returned `In_PrintError` (0xDB).
    #[error("printer error: {0}")]
    Printer(PrinterErrorCode),

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

/// Payload byte of response command `0xDB` (`In_PrintError`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrinterErrorCode {
    CoverOpen = 0x01,
    /// No paper
    LackPaper = 0x02,
    LowBattery = 0x03,
    BatteryException = 0x04,
    UserCancel = 0x05,
    DataError = 0x06,
    Overheat = 0x07,
    PaperOutException = 0x08,
    PrinterBusy = 0x09,
    NoPrinterHead = 0x0a,
    TemperatureLow = 0x0b,
    PrinterHeadLoose = 0x0c,
    NoRibbon = 0x0d,
    WrongRibbon = 0x0e,
    UsedRibbon = 0x0f,
    WrongPaper = 0x10,
    SetPaperFail = 0x11,
    SetPrintModeFail = 0x12,
    SetPrintDensityFail = 0x13,
    WriteRfidFail = 0x14,
    SetMarginFail = 0x15,
    CommunicationException = 0x16,
    Disconnect = 0x17,
    CanvasParameterError = 0x18,
    RotationParameterException = 0x19,
    JsonParameterException = 0x1a,
    B3sAbnormalPaperOutput = 0x1b,
    ECheckPaper = 0x1c,
    RfidTagNotWritten = 0x1d,
    SetPrintDensityNoSupport = 0x1e,
    SetPrintModeNoSupport = 0x1f,
    SetPrintLabelMaterialError = 0x20,
    SetPrintLabelMaterialNoSupport = 0x21,
    NotSupportWrittenRfid = 0x22,
    IllegalPage = 0x32,
    IllegalRibbonPage = 0x33,
    ReceiveDataTimeout = 0x34,
    NonDedicatedRibbon = 0x35,
    Unknown(u8),
}

impl PrinterErrorCode {
    pub fn from_u8(code: u8) -> Self {
        match code {
            0x01 => Self::CoverOpen,
            0x02 => Self::LackPaper,
            0x03 => Self::LowBattery,
            0x04 => Self::BatteryException,
            0x05 => Self::UserCancel,
            0x06 => Self::DataError,
            0x07 => Self::Overheat,
            0x08 => Self::PaperOutException,
            0x09 => Self::PrinterBusy,
            0x0a => Self::NoPrinterHead,
            0x0b => Self::TemperatureLow,
            0x0c => Self::PrinterHeadLoose,
            0x0d => Self::NoRibbon,
            0x0e => Self::WrongRibbon,
            0x0f => Self::UsedRibbon,
            0x10 => Self::WrongPaper,
            0x11 => Self::SetPaperFail,
            0x12 => Self::SetPrintModeFail,
            0x13 => Self::SetPrintDensityFail,
            0x14 => Self::WriteRfidFail,
            0x15 => Self::SetMarginFail,
            0x16 => Self::CommunicationException,
            0x17 => Self::Disconnect,
            0x18 => Self::CanvasParameterError,
            0x19 => Self::RotationParameterException,
            0x1a => Self::JsonParameterException,
            0x1b => Self::B3sAbnormalPaperOutput,
            0x1c => Self::ECheckPaper,
            0x1d => Self::RfidTagNotWritten,
            0x1e => Self::SetPrintDensityNoSupport,
            0x1f => Self::SetPrintModeNoSupport,
            0x20 => Self::SetPrintLabelMaterialError,
            0x21 => Self::SetPrintLabelMaterialNoSupport,
            0x22 => Self::NotSupportWrittenRfid,
            0x32 => Self::IllegalPage,
            0x33 => Self::IllegalRibbonPage,
            0x34 => Self::ReceiveDataTimeout,
            0x35 => Self::NonDedicatedRibbon,
            other => Self::Unknown(other),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::CoverOpen => 0x01,
            Self::LackPaper => 0x02,
            Self::LowBattery => 0x03,
            Self::BatteryException => 0x04,
            Self::UserCancel => 0x05,
            Self::DataError => 0x06,
            Self::Overheat => 0x07,
            Self::PaperOutException => 0x08,
            Self::PrinterBusy => 0x09,
            Self::NoPrinterHead => 0x0a,
            Self::TemperatureLow => 0x0b,
            Self::PrinterHeadLoose => 0x0c,
            Self::NoRibbon => 0x0d,
            Self::WrongRibbon => 0x0e,
            Self::UsedRibbon => 0x0f,
            Self::WrongPaper => 0x10,
            Self::SetPaperFail => 0x11,
            Self::SetPrintModeFail => 0x12,
            Self::SetPrintDensityFail => 0x13,
            Self::WriteRfidFail => 0x14,
            Self::SetMarginFail => 0x15,
            Self::CommunicationException => 0x16,
            Self::Disconnect => 0x17,
            Self::CanvasParameterError => 0x18,
            Self::RotationParameterException => 0x19,
            Self::JsonParameterException => 0x1a,
            Self::B3sAbnormalPaperOutput => 0x1b,
            Self::ECheckPaper => 0x1c,
            Self::RfidTagNotWritten => 0x1d,
            Self::SetPrintDensityNoSupport => 0x1e,
            Self::SetPrintModeNoSupport => 0x1f,
            Self::SetPrintLabelMaterialError => 0x20,
            Self::SetPrintLabelMaterialNoSupport => 0x21,
            Self::NotSupportWrittenRfid => 0x22,
            Self::IllegalPage => 0x32,
            Self::IllegalRibbonPage => 0x33,
            Self::ReceiveDataTimeout => 0x34,
            Self::NonDedicatedRibbon => 0x35,
            Self::Unknown(c) => c,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::CoverOpen => "CoverOpen",
            Self::LackPaper => "LackPaper",
            Self::LowBattery => "LowBattery",
            Self::BatteryException => "BatteryException",
            Self::UserCancel => "UserCancel",
            Self::DataError => "DataError",
            Self::Overheat => "Overheat",
            Self::PaperOutException => "PaperOutException",
            Self::PrinterBusy => "PrinterBusy",
            Self::NoPrinterHead => "NoPrinterHead",
            Self::TemperatureLow => "TemperatureLow",
            Self::PrinterHeadLoose => "PrinterHeadLoose",
            Self::NoRibbon => "NoRibbon",
            Self::WrongRibbon => "WrongRibbon",
            Self::UsedRibbon => "UsedRibbon",
            Self::WrongPaper => "WrongPaper",
            Self::SetPaperFail => "SetPaperFail",
            Self::SetPrintModeFail => "SetPrintModeFail",
            Self::SetPrintDensityFail => "SetPrintDensityFail",
            Self::WriteRfidFail => "WriteRfidFail",
            Self::SetMarginFail => "SetMarginFail",
            Self::CommunicationException => "CommunicationException",
            Self::Disconnect => "Disconnect",
            Self::CanvasParameterError => "CanvasParameterError",
            Self::RotationParameterException => "RotationParameterException",
            Self::JsonParameterException => "JsonParameterException",
            Self::B3sAbnormalPaperOutput => "B3sAbnormalPaperOutput",
            Self::ECheckPaper => "ECheckPaper",
            Self::RfidTagNotWritten => "RfidTagNotWritten",
            Self::SetPrintDensityNoSupport => "SetPrintDensityNoSupport",
            Self::SetPrintModeNoSupport => "SetPrintModeNoSupport",
            Self::SetPrintLabelMaterialError => "SetPrintLabelMaterialError",
            Self::SetPrintLabelMaterialNoSupport => "SetPrintLabelMaterialNoSupport",
            Self::NotSupportWrittenRfid => "NotSupportWrittenRfid",
            Self::IllegalPage => "IllegalPage",
            Self::IllegalRibbonPage => "IllegalRibbonPage",
            Self::ReceiveDataTimeout => "ReceiveDataTimeout",
            Self::NonDedicatedRibbon => "NonDedicatedRibbon",
            Self::Unknown(_) => "Unknown",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::CoverOpen => "Cover / lid is open",
            Self::LackPaper => "No paper / labels loaded",
            Self::LowBattery => "Battery too low to print",
            Self::BatteryException => "Battery fault",
            Self::UserCancel => "Print cancelled by user",
            Self::DataError => "Invalid print data",
            Self::Overheat => "Print head overheated",
            Self::PaperOutException => "Paper ran out mid-job",
            Self::PrinterBusy => "Printer is busy",
            Self::NoPrinterHead => "Print head not detected",
            Self::TemperatureLow => "Temperature too low",
            Self::PrinterHeadLoose => "Print head loose / not seated",
            Self::NoRibbon => "No ribbon installed",
            Self::WrongRibbon => "Wrong ribbon type",
            Self::UsedRibbon => "Ribbon exhausted / already used",
            Self::WrongPaper => "Wrong paper / label type",
            Self::SetPaperFail => "Failed to set paper parameters",
            Self::SetPrintModeFail => "Failed to set print mode",
            Self::SetPrintDensityFail => "Failed to set density",
            Self::WriteRfidFail => "Failed to write RFID tag on label",
            Self::SetMarginFail => "Failed to set margin",
            Self::CommunicationException => "Communication error",
            Self::Disconnect => "Disconnected during print",
            Self::CanvasParameterError => "Canvas / image parameter error",
            Self::RotationParameterException => "Invalid rotation parameter",
            Self::JsonParameterException => "Invalid JSON parameter (app-side)",
            Self::B3sAbnormalPaperOutput => "Abnormal paper output (B3S family)",
            Self::ECheckPaper => "Paper check failed",
            Self::RfidTagNotWritten => "RFID tag not written",
            Self::SetPrintDensityNoSupport => "Density setting not supported",
            Self::SetPrintModeNoSupport => "Print mode not supported",
            Self::SetPrintLabelMaterialError => "Label material setting error",
            Self::SetPrintLabelMaterialNoSupport => "Label material not supported",
            Self::NotSupportWrittenRfid => "Printer does not support writing RFID",
            Self::IllegalPage => "Illegal page / job parameter",
            Self::IllegalRibbonPage => "Illegal ribbon page parameter",
            Self::ReceiveDataTimeout => "Printer timed out waiting for data",
            Self::NonDedicatedRibbon => "Non-official / non-dedicated ribbon",
            Self::Unknown(_) => "Unknown printer error code",
        }
    }

    pub fn hint(self) -> Option<&'static str> {
        match self {
            Self::CoverOpen => Some("Close the B1 cover fully until it clicks."),
            Self::LackPaper | Self::PaperOutException | Self::ECheckPaper => {
                Some("Load a label roll with 2–5 mm sticking out of the exit slot.")
            }
            Self::LowBattery | Self::BatteryException => {
                Some("Charge the printer, then try again.")
            }
            Self::WrongPaper | Self::SetPaperFail | Self::SetPrintLabelMaterialError => {
                Some("Use compatible labels for this model; check label type in settings.")
            }
            Self::WriteRfidFail | Self::RfidTagNotWritten | Self::NotSupportWrittenRfid => {
                Some("RFID consumable issue — try official labels or a different roll.")
            }
            Self::Overheat => Some("Wait for the print head to cool, then retry."),
            Self::PrinterBusy => {
                Some("Wait for the current job to finish, or power-cycle the printer.")
            }
            Self::DataError | Self::CanvasParameterError | Self::IllegalPage => {
                Some("Check image size (B1 max width 384 px) and print-task parameters.")
            }
            _ => None,
        }
    }
}

impl fmt::Display for PrinterErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "0x{:02X} {} — {}",
            self.code(),
            self.name(),
            self.description()
        )?;
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
        let e1 = PrinterErrorCode::from_u8(0x01);
        assert_eq!(e1, PrinterErrorCode::CoverOpen);
        assert!(e1.description().contains("open"));

        let e2 = PrinterErrorCode::from_u8(0x02);
        assert_eq!(e2, PrinterErrorCode::LackPaper);
        assert!(e2.description().to_lowercase().contains("paper"));
    }

    #[test]
    fn from_u8_roundtrip_known_and_unknown() {
        for code in [0x01u8, 0x02, 0x14, 0x35, 0x99] {
            let e = PrinterErrorCode::from_u8(code);
            assert_eq!(e.code(), code);
            let _ = e.to_string();
            let _ = e.hint();
        }
    }

    #[test]
    fn typed_error_is_matchable() {
        let err = Error::Printer(PrinterErrorCode::LackPaper);
        match err {
            Error::Printer(PrinterErrorCode::LackPaper) => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn packet_error_converts() {
        let err: Error = PacketError::BadHead.into();
        assert!(matches!(err, Error::Packet(_)));
    }
}
