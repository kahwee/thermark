//! Command IDs and high-level request helpers for the NIIMBOT protocol.

use crate::packet::Packet;

/// Request command codes (host → printer).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    PrintStart = 0x01,
    PageStart = 0x03,
    SetPageSize = 0x13,
    PrintQuantity = 0x15,
    RfidInfo = 0x1a,
    PrintClear = 0x20,
    SetDensity = 0x21,
    SetLabelType = 0x23,
    PrinterInfo = 0x40,
    PrintBitmapRowIndexed = 0x83,
    PrintEmptyRow = 0x84,
    PrintBitmapRow = 0x85,
    PrintStatus = 0xa3,
    Connect = 0xc1,
    CancelPrint = 0xda,
    Heartbeat = 0xdc,
    PageEnd = 0xe3,
    PrintEnd = 0xf3,
}

/// Printer info keys for `PrinterInfo` (0x40).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoKey {
    Density = 1,
    PrintSpeed = 2,
    LabelType = 3,
    LanguageType = 6,
    AutoShutdownTime = 7,
    DeviceType = 8,
    SoftVersion = 9,
    Battery = 10,
    DeviceSerial = 11,
    HardVersion = 12,
}

/// Supported printer models (printhead width and print-start variants).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Hash,
    clap::ValueEnum,
    serde::Serialize,
    serde::Deserialize,
)]
#[value(rename_all = "lower")]
#[serde(rename_all = "lowercase")]
pub enum Model {
    #[default]
    B1,
    B21,
    B18,
    D11,
    D110,
}

impl Model {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "b1" => Some(Self::B1),
            "b21" => Some(Self::B21),
            "b18" => Some(Self::B18),
            "d11" => Some(Self::D11),
            "d110" => Some(Self::D110),
            _ => None,
        }
    }

    /// Max printable width in pixels (~203 dpi / 8 px per mm).
    ///
    /// The effective limit for a job is this *and* the print task's — use
    /// [`crate::print_task::effective_max_width_px`].
    pub fn max_width_px(self) -> u32 {
        match self {
            Self::B1 | Self::B21 | Self::B18 => crate::geometry::HEAD_WIDE_PX,
            Self::D11 | Self::D110 => crate::geometry::HEAD_NARROW_PX,
        }
    }

    /// Payload for PrintStart. B1/newer may prefer the 7-byte form; simple `01`
    /// is widely accepted (the simple print-task form tested on B1).
    pub fn print_start_payload(self) -> Vec<u8> {
        match self {
            // 7-byte form from community wiki (total pages = 1)
            Self::B1 => vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00],
            // 1-byte form used by older models / the simple print-task form fallback
            _ => vec![0x01],
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown model '{0}' (try b1, b21, d11, d110, b18)")]
pub struct ModelParseError(pub String);

impl std::str::FromStr for Model {
    type Err = ModelParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| ModelParseError(s.to_string()))
    }
}

impl Model {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::B1 => "b1",
            Self::B21 => "b21",
            Self::B18 => "b18",
            Self::D11 => "d11",
            Self::D110 => "d110",
        }
    }
}

impl std::fmt::Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// Expected response command for a request (default: request + 1).
pub fn default_response_cmd(request: u8) -> u8 {
    request.wrapping_add(1)
}

pub fn pkt(cmd: Cmd, data: impl Into<Vec<u8>>) -> Packet {
    Packet::new(cmd as u8, data)
}

pub fn info(key: InfoKey) -> Packet {
    pkt(Cmd::PrinterInfo, vec![key as u8])
}

pub fn heartbeat() -> Packet {
    pkt(Cmd::Heartbeat, vec![0x01])
}

pub fn rfid() -> Packet {
    pkt(Cmd::RfidInfo, vec![0x01])
}

pub fn set_density(level: u8) -> Packet {
    pkt(Cmd::SetDensity, vec![level])
}

pub fn set_label_type(t: u8) -> Packet {
    pkt(Cmd::SetLabelType, vec![t])
}

pub fn print_start(model: Model) -> Packet {
    pkt(Cmd::PrintStart, model.print_start_payload())
}

/// Fallback simple print-start used by the simple print-task form (works on many firmwares).
pub fn print_start_simple() -> Packet {
    pkt(Cmd::PrintStart, vec![0x01])
}

pub fn page_start() -> Packet {
    pkt(Cmd::PageStart, vec![0x01])
}

pub fn page_end() -> Packet {
    pkt(Cmd::PageEnd, vec![0x01])
}

pub fn print_end() -> Packet {
    pkt(Cmd::PrintEnd, vec![0x01])
}

pub fn cancel_print() -> Packet {
    pkt(Cmd::CancelPrint, vec![0x01])
}

/// Set page size: row count (height) and column count (width), big-endian.
pub fn set_page_size(rows: u16, cols: u16) -> Packet {
    let mut data = Vec::with_capacity(4);
    data.extend_from_slice(&rows.to_be_bytes());
    data.extend_from_slice(&cols.to_be_bytes());
    pkt(Cmd::SetPageSize, data)
}

/// B1 print-task page size: rows, cols, copies (all u16 BE).
pub fn set_page_size_b1(rows: u16, cols: u16, copies: u16) -> Packet {
    let mut data = Vec::with_capacity(6);
    data.extend_from_slice(&rows.to_be_bytes());
    data.extend_from_slice(&cols.to_be_bytes());
    data.extend_from_slice(&copies.to_be_bytes());
    pkt(Cmd::SetPageSize, data)
}

pub fn set_quantity(n: u16) -> Packet {
    pkt(Cmd::PrintQuantity, n.to_be_bytes().to_vec())
}

pub fn print_status() -> Packet {
    pkt(Cmd::PrintStatus, vec![0x01])
}

/// Encode one bitmap print row (command 0x85).
///
/// Layout: `row_index:u16be | black_counts:3bytes | repeats:u8 | pixels…`
/// Community clients often send black_counts as zeros successfully.
pub fn print_bitmap_row(row_index: u16, repeats: u8, pixels: &[u8]) -> Packet {
    let mut data = Vec::with_capacity(6 + pixels.len());
    data.extend_from_slice(&row_index.to_be_bytes());
    data.extend_from_slice(&[0, 0, 0]); // black pixel counts (optional / ignored)
    data.push(repeats);
    data.extend_from_slice(pixels);
    pkt(Cmd::PrintBitmapRow, data)
}

pub fn print_empty_row(row_index: u16, repeats: u8) -> Packet {
    let mut data = Vec::with_capacity(3);
    data.extend_from_slice(&row_index.to_be_bytes());
    data.push(repeats);
    pkt(Cmd::PrintEmptyRow, data)
}
