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

impl std::fmt::Display for RfidInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.tag_present {
            return write!(f, "(no RFID tag)");
        }
        write!(
            f,
            "barcode={} serial={} paper={}/{} type={} uuid={}",
            self.barcode,
            self.serial,
            self.used_paper,
            self.all_paper,
            self.consumables_type,
            self.uuid_hex
        )?;
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
    pub fn print_blocker(&self) -> Option<crate::errors::PrinterErrorCode> {
        use crate::errors::PrinterErrorCode;
        if self.closing_state == Some(1) {
            return Some(PrinterErrorCode::CoverOpen);
        }
        if self.paper_state == Some(1) {
            return Some(PrinterErrorCode::LackPaper);
        }
        if self.power_level == Some(0) {
            return Some(PrinterErrorCode::LowBattery);
        }
        None
    }
}

#[cfg(test)]
mod heartbeat_blocker_tests {
    use super::*;
    use crate::errors::PrinterErrorCode;

    #[test]
    fn print_blocker_cover_paper_battery() {
        let mut hb = Heartbeat::default();
        assert!(hb.print_blocker().is_none());

        hb.closing_state = Some(1);
        assert_eq!(hb.print_blocker(), Some(PrinterErrorCode::CoverOpen));

        hb.closing_state = Some(0);
        hb.paper_state = Some(1);
        assert_eq!(hb.print_blocker(), Some(PrinterErrorCode::LackPaper));

        hb.paper_state = Some(0);
        hb.power_level = Some(0);
        assert_eq!(hb.print_blocker(), Some(PrinterErrorCode::LowBattery));
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
            writeln!(f, "  battery:      {v}")?;
        }
        if let Some(r) = &self.rfid {
            writeln!(f, "  RFID:         {r}")?;
            if !r.barcode.is_empty() {
                writeln!(
                    f,
                    "  tip: barcode often encodes label size — use --label matching your roll"
                )?;
            }
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
