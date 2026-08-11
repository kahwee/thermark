//! Parsed printer info, heartbeat, and RFID payloads.

use crate::protocol::InfoKey;

#[derive(Debug, Clone, Default)]
pub struct RfidInfo {
    pub tag_present: bool,
    pub uuid_hex: String,
    pub barcode: String,
    pub serial: String,
    pub all_paper: i16,
    pub used_paper: i16,
    pub consumables_type: u8,
    pub capacity: Option<i16>,
}

impl RfidInfo {
    pub(crate) fn parse(data: &[u8]) -> Self {
        if data.len() <= 1 {
            return Self {
                tag_present: false,
                ..Default::default()
            };
        }
        let mut i = 0usize;
        let mut out = Self {
            tag_present: true,
            ..Default::default()
        };
        if data.len() >= 8 {
            out.uuid_hex = hex::encode(&data[0..8]);
            i = 8;
        }
        if i < data.len() {
            let n = data[i] as usize;
            i += 1;
            if i + n <= data.len() {
                out.barcode = String::from_utf8_lossy(&data[i..i + n]).into_owned();
                i += n;
            }
        }
        if i < data.len() {
            let n = data[i] as usize;
            i += 1;
            if i + n <= data.len() {
                out.serial = String::from_utf8_lossy(&data[i..i + n]).into_owned();
                i += n;
            }
        }
        if i + 4 <= data.len() {
            out.all_paper = i16::from_be_bytes([data[i], data[i + 1]]);
            out.used_paper = i16::from_be_bytes([data[i + 2], data[i + 3]]);
            i += 4;
        }
        if i < data.len() {
            out.consumables_type = data[i];
            i += 1;
        }
        if i + 2 <= data.len() {
            out.capacity = Some(i16::from_be_bytes([data[i], data[i + 1]]));
        }
        out
    }
}

/// Consumable type as reported by the roll's RFID tag (`consumables_type`).
///
/// This is *what kind* of stock is loaded, not how big it is — the tag carries
/// no geometry at all. See the paper-size FAQ in the README.
pub fn consumable_type_name(code: u8) -> &'static str {
    match code {
        0 => "invalid",
        1 => "gapped",
        2 => "black-mark",
        3 => "continuous",
        4 => "perforated",
        5 => "transparent",
        6 => "pvc-tag",
        10 => "black-mark-gap",
        11 => "heat-shrink",
        _ => "unknown",
    }
}

impl RfidInfo {
    /// Human-readable consumable type.
    pub fn consumable_type_name(&self) -> &'static str {
        consumable_type_name(self.consumables_type)
    }

    /// Labels left on the roll, when the tag reports both counts.
    ///
    /// Negative counts mean "not reported" — the parser leaves the `-1`
    /// sentinel the printer sends rather than inventing a zero.
    pub fn labels_remaining(&self) -> Option<i16> {
        (self.all_paper >= 0 && self.used_paper >= 0)
            .then(|| (self.all_paper - self.used_paper).max(0))
    }
}

impl std::fmt::Display for RfidInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.tag_present {
            return write!(f, "(no RFID tag)");
        }
        write!(
            f,
            "barcode={} serial={} paper={}/{} type={} ({}) uuid={}",
            self.barcode,
            self.serial,
            self.used_paper,
            self.all_paper,
            self.consumables_type,
            self.consumable_type_name(),
            self.uuid_hex
        )?;
        if let Some(left) = self.labels_remaining() {
            write!(f, " remaining={left}")?;
        }
        if let Some(c) = self.capacity {
            write!(f, " capacity={c}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum InfoValue {
    Int(u64),
    Float(f64),
    /// Printable ASCII (device serial strings, etc.).
    Text(String),
    /// Opaque bytes as hex.
    Hex(String),
    Raw(Vec<u8>),
}

impl InfoValue {
    pub(crate) fn parse(key: InfoKey, data: &[u8]) -> Self {
        match key {
            InfoKey::DeviceSerial => {
                if !data.is_empty() && data.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
                    Self::Text(String::from_utf8_lossy(data).into_owned())
                } else {
                    Self::Hex(hex::encode(data))
                }
            }
            InfoKey::SoftVersion | InfoKey::HardVersion => {
                let n = be_int(data);
                Self::Float(n as f64 / 100.0)
            }
            _ => Self::Int(be_int(data)),
        }
    }
}

impl std::fmt::Display for InfoValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v:.2}"),
            Self::Text(s) | Self::Hex(s) => write!(f, "{s}"),
            Self::Raw(b) => write!(f, "{}", hex::encode(b)),
        }
    }
}

fn be_int(data: &[u8]) -> u64 {
    data.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b))
}

#[derive(Debug, Clone, Default)]
pub struct Heartbeat {
    pub closing_state: Option<u8>,
    pub power_level: Option<u8>,
    pub paper_state: Option<u8>,
    pub rfid_read_state: Option<u8>,
    pub raw_len: usize,
}

impl Heartbeat {
    pub(crate) fn parse(data: &[u8]) -> Self {
        let mut hb = Self {
            raw_len: data.len(),
            ..Default::default()
        };
        match data.len() {
            20 => {
                hb.paper_state = data.get(18).copied();
                hb.rfid_read_state = data.get(19).copied();
            }
            13 => {
                hb.closing_state = data.get(9).copied();
                hb.power_level = data.get(10).copied();
                hb.paper_state = data.get(11).copied();
                hb.rfid_read_state = data.get(12).copied();
            }
            19 => {
                hb.closing_state = data.get(15).copied();
                hb.power_level = data.get(16).copied();
                hb.paper_state = data.get(17).copied();
                hb.rfid_read_state = data.get(18).copied();
            }
            10 => {
                hb.closing_state = data.get(8).copied();
                hb.power_level = data.get(9).copied();
            }
            9 => {
                hb.closing_state = data.get(8).copied();
            }
            _ => {}
        }
        hb
    }

    /// Hard preflight conditions that should abort a print job.
    ///
    /// Matches [`crate::doctor::evaluate_heartbeat`] FAIL cases for cover, paper,
    /// and empty battery. Missing fields are not blockers (firmware-dependent).
    pub fn print_blocker(&self) -> Option<crate::errors::PrinterFault> {
        use crate::errors::PrinterFault;
        if self.closing_state == Some(1) {
            return Some(PrinterFault::COVER_OPEN);
        }
        if self.paper_state == Some(1) {
            return Some(PrinterFault::NO_PAPER);
        }
        if self.power_level == Some(0) {
            return Some(PrinterFault::LOW_BATTERY);
        }
        None
    }
}

#[cfg(test)]
mod heartbeat_blocker_tests {
    use super::*;
    use crate::errors::PrinterFault;

    #[test]
    fn print_blocker_cover_paper_battery() {
        let mut hb = Heartbeat::default();
        assert!(hb.print_blocker().is_none());

        hb.closing_state = Some(1);
        assert_eq!(hb.print_blocker(), Some(PrinterFault::COVER_OPEN));

        hb.closing_state = Some(0);
        hb.paper_state = Some(1);
        assert_eq!(hb.print_blocker(), Some(PrinterFault::NO_PAPER));

        hb.paper_state = Some(0);
        hb.power_level = Some(0);
        assert_eq!(hb.print_blocker(), Some(PrinterFault::LOW_BATTERY));
    }
}

/// Highest power level the printer reports. The scale is 0..=`BATTERY_MAX`.
pub const BATTERY_MAX: u8 = 4;

/// Plain-language reading of a power level.
///
/// One source of truth so `info` and `doctor` cannot describe the same battery
/// differently. A bare "battery: 1" reads like a unit, not a warning — and a
/// low battery makes dense pages print only partway, which is easy to mistake
/// for a layout bug.
pub fn describe_battery(level: u8) -> String {
    let meaning = match level {
        0 => "empty — will not print",
        1 => "low — dense or dark labels may print only partway; charge it",
        2 => "about half",
        3 => "good",
        _ => "full",
    };
    format!("{level}/{BATTERY_MAX}  ({meaning})")
}

/// Same, for a value that may not parse as a level.
pub fn describe_battery_str(raw: &str) -> String {
    match raw.trim().parse::<u8>() {
        Ok(level) if level <= BATTERY_MAX => describe_battery(level),
        _ => raw.to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct PrinterSummary {
    pub serial: Option<InfoValue>,
    pub soft: Option<InfoValue>,
    pub hard: Option<InfoValue>,
    pub battery: Option<InfoValue>,
    pub device_type: Option<InfoValue>,
    pub heartbeat: Option<Heartbeat>,
    pub rfid: Option<RfidInfo>,
}

impl PrinterSummary {
    pub fn has_data(&self) -> bool {
        self.serial.is_some()
            || self.soft.is_some()
            || self.hard.is_some()
            || self.battery.is_some()
            || self.device_type.is_some()
            || self.heartbeat.is_some()
            || self.rfid.is_some()
    }
}

impl std::fmt::Display for PrinterSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Printer info")?;
        if let Some(v) = &self.serial {
            writeln!(f, "  serial:       {v}")?;
        }
        if let Some(v) = &self.device_type {
            writeln!(f, "  device type:  {v}")?;
        }
        if let Some(v) = &self.soft {
            writeln!(f, "  soft version: {v}")?;
        }
        if let Some(v) = &self.hard {
            writeln!(f, "  hard version: {v}")?;
        }
        if let Some(v) = &self.battery {
            writeln!(
                f,
                "  battery:      {}",
                describe_battery_str(&v.to_string())
            )?;
        }
        if let Some(r) = &self.rfid {
            if r.tag_present {
                write!(f, "  paper:        {}", r.consumable_type_name())?;
                if let Some(left) = r.labels_remaining() {
                    write!(f, ", {left} of {} labels left", r.all_paper)?;
                }
                writeln!(f)?;
            } else {
                writeln!(f, "  paper:        no RFID tag (size and type unknown)")?;
            }
            writeln!(f, "  RFID:         {r}")?;
            // The tag has no width or height on it — only vendor software maps
            // the barcode to a size. Say so, rather than implying the printer
            // could tell thermark what is loaded.
            writeln!(
                f,
                "  note: the tag carries no label size — pass --label WxH for your roll"
            )?;
        }
        if let Some(hb) = &self.heartbeat {
            writeln!(f, "  heartbeat ({} bytes):", hb.raw_len)?;
            if let Some(v) = hb.power_level {
                writeln!(f, "    power:  {v}")?;
            }
            if let Some(v) = hb.closing_state {
                writeln!(f, "    lid:    {v}  (0=closed on most models)")?;
            }
            if let Some(v) = hb.paper_state {
                writeln!(f, "    paper:  {v}  (0=inserted on most models)")?;
            }
            if let Some(v) = hb.rfid_read_state {
                writeln!(f, "    rfid:   {v}  (1=RFID ok)")?;
            }
        }
        writeln!(
            f,
            "  geometry:     8 px/mm (~203 dpi), B1 max width 384 px (~48 mm)"
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_serial_ascii_is_text() {
        let v = InfoValue::parse(InfoKey::DeviceSerial, b"TESTMOCK01");
        match v {
            InfoValue::Text(s) => assert_eq!(s, "TESTMOCK01"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn info_serial_binary_is_hex() {
        let v = InfoValue::parse(InfoKey::DeviceSerial, &[0x00, 0xff]);
        match v {
            InfoValue::Hex(s) => assert_eq!(s, "00ff"),
            other => panic!("expected Hex, got {other:?}"),
        }
    }

    #[test]
    fn rfid_empty_tag() {
        let r = RfidInfo::parse(&[0x00]);
        assert!(!r.tag_present);
    }
}

/// Parsed `PrintStatus` (0xa3) reply.
///
/// thermark used to poll this purely as a keepalive and throw the payload away.
/// It answers the question that cost a whole debugging session — *how far did
/// the printer actually get?* A page that stops at 73 % because the battery
/// sagged reports `page_print_progress: 73`, where the discarded reply looked
/// exactly like a successful one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrintStatus {
    /// Pages completed so far.
    pub page: i16,
    /// Percent of the current page imaged (0-100).
    pub page_print_progress: u8,
    /// Percent of the current page fed out (0-100).
    pub page_feed_progress: u8,
    /// Fault code, present only in the 10-byte form of the reply. Non-zero
    /// means the printer named a specific problem.
    pub error: Option<u8>,
}

impl PrintStatus {
    /// Parse the reply body. Needs at least 4 bytes; the 10-byte form carries
    /// an extra fault code at offset 6, which the shorter forms omit entirely.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let [p0, p1, print, feed, ..] = *data else {
            return None;
        };
        Some(Self {
            page: i16::from_be_bytes([p0, p1]),
            page_print_progress: print,
            page_feed_progress: feed,
            // Only the 10-byte form has it. Reading `data.get(6)` on any
            // length would invent a fault out of some other field.
            error: (data.len() == 10).then(|| data[6]).filter(|&e| e != 0),
        })
    }

    /// Both the imaging and feed passes reported complete.
    pub fn page_complete(self) -> bool {
        self.page_print_progress >= 100 && self.page_feed_progress >= 100
    }
}

impl std::fmt::Display for PrintStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "page={} print={}% feed={}%",
            self.page, self.page_print_progress, self.page_feed_progress
        )?;
        if let Some(e) = self.error {
            write!(f, " error=0x{e:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod print_status_tests {
    use super::PrintStatus;

    #[test]
    fn parses_the_short_form_without_inventing_a_fault() {
        let s = PrintStatus::parse(&[0, 1, 100, 100]).unwrap();
        assert_eq!(s.page, 1);
        assert!(s.page_complete());
        assert_eq!(s.error, None, "short form has no fault byte to read");
    }

    #[test]
    fn reads_the_fault_code_only_from_the_ten_byte_form() {
        // Offset 6 is 0x0e here. In the 10-byte form that is a fault; at any
        // other length it is some other field and must not be reported.
        let body = [0, 1, 73, 0, 0, 0, 0x0e, 0, 0, 0];
        let s = PrintStatus::parse(&body).unwrap();
        assert_eq!(s.error, Some(0x0e));
        assert_eq!(s.page_print_progress, 73);
        assert!(!s.page_complete());

        let s = PrintStatus::parse(&body[..8]).unwrap();
        assert_eq!(s.error, None);
    }

    #[test]
    fn zero_fault_code_is_not_a_fault() {
        let s = PrintStatus::parse(&[0, 1, 100, 100, 0, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(s.error, None);
    }

    #[test]
    fn rejects_a_truncated_reply() {
        assert!(PrintStatus::parse(&[0, 1, 100]).is_none());
        assert!(PrintStatus::parse(&[]).is_none());
    }
}

#[cfg(test)]
mod rfid_tests {
    use super::RfidInfo;

    fn tag(all: i16, used: i16, kind: u8) -> RfidInfo {
        RfidInfo {
            tag_present: true,
            all_paper: all,
            used_paper: used,
            consumables_type: kind,
            ..Default::default()
        }
    }

    #[test]
    fn reports_labels_left_on_the_roll() {
        assert_eq!(tag(200, 37, 1).labels_remaining(), Some(163));
        // Used > all happens on a re-fitted roll; clamp rather than go negative.
        assert_eq!(tag(200, 250, 1).labels_remaining(), Some(0));
    }

    #[test]
    fn unreported_counts_stay_unreported() {
        // The printer sends -1 for "no count". Turning that into 0 would claim
        // the roll is empty.
        assert_eq!(tag(-1, -1, 1).labels_remaining(), None);
        assert_eq!(tag(200, -1, 1).labels_remaining(), None);
    }

    #[test]
    fn names_the_consumable_type() {
        assert_eq!(tag(0, 0, 1).consumable_type_name(), "gapped");
        assert_eq!(tag(0, 0, 3).consumable_type_name(), "continuous");
        assert_eq!(tag(0, 0, 99).consumable_type_name(), "unknown");
    }
}
