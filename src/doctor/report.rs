use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Pass => "ok",
            Self::Warn => "warn",
            Self::Fail => "FAIL",
        }
    }

    pub fn worse(self, other: Self) -> Self {
        use CheckStatus::*;
        match (self, other) {
            (Fail, _) | (_, Fail) => Fail,
            (Warn, _) | (_, Warn) => Warn,
            _ => Pass,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
}

impl Check {
    pub fn new(name: impl Into<String>, status: CheckStatus, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
        }
    }

    pub fn pass(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Pass, detail)
    }

    pub fn warn(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Warn, detail)
    }

    pub fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Fail, detail)
    }
}

/// Classify one optional sensor reading into a [`Check`].
///
/// The heartbeat sensors differ only in which value is good and what to say
/// about it, so the `Option<u8>` match shape lives here once. `classify`
/// returning `None` means "value outside the documented range".
pub(super) fn sensor_check(
    name: &'static str,
    value: Option<u8>,
    classify: impl Fn(u8) -> Option<(CheckStatus, String)>,
) -> Check {
    match value {
        Some(v) => match classify(v) {
            Some((status, detail)) => Check::new(name, status, detail),
            None => Check::warn(name, format!("unexpected {name} state {v}")),
        },
        None => Check::warn(name, "not reported in heartbeat"),
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{:<4}] {:<22} {}",
            self.status.icon(),
            self.name,
            self.detail
        )
    }
}

#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

#[derive(Serialize)]
struct SupportCheck {
    name: &'static str,
    status: CheckStatus,
}

#[derive(Serialize)]
struct SupportReport {
    schema_version: u8,
    thermark_version: &'static str,
    os: &'static str,
    arch: &'static str,
    features: Vec<&'static str>,
    overall: CheckStatus,
    privacy: &'static str,
    checks: Vec<SupportCheck>,
}

/// A report may be assembled by library callers, so even check names cannot
/// be trusted as public-safe text. Unknown names keep their status under a
/// fixed label rather than passing arbitrary strings into the JSON.
fn support_check_name(name: &str) -> &'static str {
    match name {
        "thermark" => "thermark",
        "print_task" => "print_task",
        "fonts" => "fonts",
        "serial_ports" => "serial_ports",
        "bluetooth" => "bluetooth",
        "ble_scan" => "ble_scan",
        "ble_session" => "ble_session",
        "ble_connect" => "ble_connect",
        "ble_disconnect" => "ble_disconnect",
        "heartbeat" => "heartbeat",
        "cover" => "cover",
        "paper" => "paper",
        "rfid" => "rfid",
        "battery" => "battery",
        "rfid_tag" => "rfid_tag",
        "serial" => "serial",
        "support_matrix" => "support_matrix",
        "usb_open" => "usb_open",
        "usb" => "usb",
        _ => "other",
    }
}

impl DoctorReport {
    pub fn overall(&self) -> CheckStatus {
        self.checks
            .iter()
            .map(|c| c.status)
            .fold(CheckStatus::Pass, CheckStatus::worse)
    }

    pub fn exit_code(&self) -> i32 {
        match self.overall() {
            CheckStatus::Pass | CheckStatus::Warn => 0,
            CheckStatus::Fail => 1,
        }
    }

    /// Serialize a support report that is safe to attach to a public issue.
    ///
    /// Check names and details are public library data, so only canonical
    /// names and statuses enter this report. The human-readable doctor output
    /// retains the full detail for local diagnosis.
    pub fn support_json_pretty(&self) -> serde_json::Result<String> {
        let features = [
            ("ble", cfg!(feature = "ble")),
            ("serial", cfg!(feature = "serial")),
        ]
        .into_iter()
        .filter_map(|(name, enabled)| enabled.then_some(name))
        .collect();

        let checks = self
            .checks
            .iter()
            .map(|check| SupportCheck {
                name: support_check_name(&check.name),
                status: check.status,
            })
            .collect();

        serde_json::to_string_pretty(&SupportReport {
            schema_version: 1,
            thermark_version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            features,
            overall: self.overall(),
            privacy: "printer identifiers and local paths omitted",
            checks,
        })
    }
}

impl fmt::Display for DoctorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "thermark doctor")?;
        writeln!(f, "---------------")?;
        for c in &self.checks {
            writeln!(f, "{c}")?;
        }
        writeln!(f)?;
        match self.overall() {
            CheckStatus::Pass => writeln!(f, "Overall: ok — ready to print (if labels loaded)"),
            CheckStatus::Warn => {
                writeln!(f, "Overall: warnings — printing may work; review above")
            }
            CheckStatus::Fail => writeln!(f, "Overall: FAIL — fix failures before printing"),
        }
    }
}
