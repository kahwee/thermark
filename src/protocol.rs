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
    /// Sparse-row form: 2-byte pixel indices instead of a bitmap.
    ///
    /// The reference implementation switches to it whenever a row has **≤ 6**
    /// black pixels, and its source carries the note *"printer powers off if
    /// black pixel count > 6"* — i.e. this is not a size optimisation but a
    /// firmware quirk with a hard threshold. It refuses to build the packet
    /// above 6, so the two forms partition rows rather than overlap.
    ///
    /// Not emitted here, and no such power-off has been observed. Worth
    /// implementing before printing artwork made of hairlines — a 1 px rule or
    /// a thin border is exactly the row that lands under the threshold.
    PrintBitmapRowIndexed = 0x83,
    PrintEmptyRow = 0x84,
    PrintBitmapRow = 0x85,
    /// `[line: u16, 0x01]`, reply `0xd3`. The reference implementation marks a
    /// slot every 200 rows (`row % 200 == 199`) but emits the packet **only
    /// under an opt-in `enableCheckLine` flag, off by default** — so it is not
    /// load-bearing for reliability, on long pages or otherwise. Not emitted
    /// here, deliberately.
    PrinterCheckLine = 0x86,
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
///
/// **Authoring orientation differs by profile.** The reference implementation
/// tags each model with a print direction; B18 and the D11/D110 family are
/// `"left"` and have their canvas rotated 90° clockwise during
/// encoding, so its *long* edge is the one designed across. thermark does not
/// rotate — a D110 label is authored as `--label 12x40`, narrow side first, and
/// the bytes on the wire come out identical. Use `--rotate` if artwork was
/// drawn the other way round.
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
    B1Pro,
    B21Pro,
    B18,
    D11,
    D11H,
    D110,
}

impl Model {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::B1 => "b1",
            Self::B1Pro => "b1pro",
            Self::B21Pro => "b21pro",
            Self::B18 => "b18",
            Self::D11 => "d11",
            Self::D11H => "d11h",
            Self::D110 => "d110",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "b1" => Some(Self::B1),
            "b1pro" | "b1_pro" => Some(Self::B1Pro),
            "b21pro" | "b21_pro" => Some(Self::B21Pro),
            "b18" => Some(Self::B18),
            "d11" => Some(Self::D11),
            "d11h" | "d11_h" => Some(Self::D11H),
            "d110" => Some(Self::D110),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown model '{0}' (try b1, b1pro, b21pro, b18, d11, d11h, d110)")]
pub struct ModelParseError(pub String);

impl std::str::FromStr for Model {
    type Err = ModelParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| ModelParseError(s.to_string()))
    }
}

impl std::fmt::Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

pub fn pkt(cmd: Cmd, data: impl Into<Vec<u8>>) -> Packet {
    Packet::new(cmd as u8, data)
}

pub fn info(key: InfoKey) -> Packet {
    pkt(Cmd::PrinterInfo, vec![key as u8])
}

pub fn connect() -> Packet {
    pkt(Cmd::Connect, vec![0x01])
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

pub fn print_end() -> Packet {
    pkt(Cmd::PrintEnd, vec![0x01])
}

pub fn cancel_print() -> Packet {
    pkt(Cmd::CancelPrint, vec![0x01])
}

pub fn print_status() -> Packet {
    pkt(Cmd::PrintStatus, vec![0x01])
}

pub fn printer_status_data() -> Packet {
    Packet::new(0xa5, vec![0x01])
}

/// Encode one bitmap print row (command 0x85).
///
/// Layout: `row_index:u16be | black_counts:3bytes | repeats:u8 | pixels…`
///
/// `black_counts` is sent as zeros. The reference implementation computes real
/// counts against the printhead width (`auto | split | total`); zeros are
/// widely reported to work and do print here, so this is a deliberate
/// deviation rather than an oversight.
///
/// `repeats` coalesces consecutive identical rows. It is one byte, so callers
/// must split longer runs at 255; [`crate::image_encode::encode`] does this.
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
