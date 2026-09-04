//! Environment + printer readiness checks (`thermark doctor`).

use crate::config::ConnPref;
use crate::errors::Result;
use crate::print_task::{PrintTask, SupportStatus, hardware_matrix};
use crate::printer::{Heartbeat, RfidInfo};
use crate::protocol::Model;
use std::fmt;

use crate::transport::BleMatchMode;
// Only a transport actually talks to a printer, so with both features off these
// are dead. Gated rather than `#[allow(unused)]` so a genuinely unused import
// still gets reported.
#[cfg(any(feature = "ble", feature = "serial"))]
use crate::printer::PrinterClient;
#[cfg(feature = "ble")]
use std::time::Duration;

#[cfg(feature = "serial")]
use crate::transport::SerialTransport;
#[cfg(feature = "ble")]
use crate::transport::{self, BleTransport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
fn sensor_check(
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

/// Interpret heartbeat for readiness (shared with tests).
pub fn evaluate_heartbeat(hb: &Heartbeat) -> Vec<Check> {
    use CheckStatus::{Fail, Pass, Warn};
    vec![
        sensor_check("cover", hb.closing_state, |v| match v {
            0 => Some((Pass, "closed (0)".into())),
            1 => Some((Fail, "open (1) — close the lid fully".into())),
            _ => None,
        }),
        sensor_check("paper", hb.paper_state, |v| match v {
            0 => Some((Pass, "detected (0)".into())),
            1 => Some((Fail, "not detected (1) — load labels, 2–5mm out".into())),
            _ => None,
        }),
        sensor_check("rfid", hb.rfid_read_state, |v| match v {
            1 => Some((Pass, "ok (1)".into())),
            0 => Some((
                Warn,
                "not ok (0) — third-party media may print poorly".into(),
            )),
            _ => None,
        }),
        sensor_check("battery", hb.power_level, |v| {
            // Wording comes from `describe_battery` so info and doctor agree.
            let text = crate::printer::describe_battery(v);
            match v {
                0 => Some((Fail, text)),
                1 => Some((Warn, text)),
                _ => Some((Pass, text)),
            }
        }),
    ]
}

pub fn evaluate_rfid(r: &RfidInfo) -> Check {
    if !r.tag_present {
        return Check::warn("rfid_tag", "no RFID tag data (ok for some media)");
    }
    Check::pass(
        "rfid_tag",
        format!(
            "barcode={} paper={}/{} type={}",
            if r.barcode.is_empty() {
                "-"
            } else {
                &r.barcode
            },
            r.used_paper,
            r.all_paper,
            r.consumables_type
        ),
    )
}

/// Report the task a print would actually use, honouring an explicit `--task`.
pub fn evaluate_print_task(model: Model, task: Option<PrintTask>) -> Check {
    let Some(task) = task.or_else(|| PrintTask::for_model(model)) else {
        return Check::warn(
            "print task",
            format!("model={model} has no verified default task"),
        );
    };
    let profile = crate::profile::profile_for_model(model);
    let (status, label) = if profile.print_path_hardware_tested(task) {
        (CheckStatus::Pass, "hardware-tested")
    } else {
        (CheckStatus::Warn, "experimental")
    };
    Check::new(
        "print_task",
        status,
        format!(
            "model={model} → task={task} ({label}), max width {}px",
            profile.max_width_px
        ),
    )
}

/// What to diagnose. `addr: None` means host-only checks (no connect).
#[derive(Debug, Clone, Default)]
pub struct DoctorOptions {
    pub addr: Option<String>,
    pub model: Model,
    pub task: Option<PrintTask>,
    pub scan_secs: u64,
    pub conn: ConnPref,
    pub match_mode: BleMatchMode,
}

/// Run diagnostic checks. Without an address, only host + scan checks run.
pub async fn run_doctor(opts: &DoctorOptions) -> Result<DoctorReport> {
    let DoctorOptions {
        model,
        scan_secs,
        match_mode,
        ..
    } = *opts;
    let addr = opts.addr.as_deref();
    let mut checks = Vec::new();

    // Build / crate
    checks.push(Check::pass(
        "thermark",
        format!("v{}", env!("CARGO_PKG_VERSION")),
    ));

    // Print task honesty
    checks.push(evaluate_print_task(model, opts.task));

    // Fonts
    let fonts = crate::font::list_available_fonts();
    if fonts.is_empty() {
        checks.push(Check::warn(
            "fonts",
            "no system TTF candidates found — pass --font",
        ));
    } else {
        checks.push(Check::pass(
            "fonts",
            format!("{} candidate(s), e.g. {}", fonts.len(), fonts[0].display()),
        ));
    }

    // Serial ports (informational)
    #[cfg(feature = "serial")]
    match SerialTransport::list_ports() {
        Ok(ports) if ports.is_empty() => checks.push(Check::warn(
            "serial_ports",
            "none listed (BLE-only is fine)",
        )),
        Ok(ports) => checks.push(Check::pass(
            "serial_ports",
            format!("{} port(s): {}", ports.len(), ports.join(", ")),
        )),
        Err(e) => checks.push(Check::warn("serial_ports", format!("could not list: {e}"))),
    }
    #[cfg(not(feature = "serial"))]
    checks.push(Check::warn(
        "serial_ports",
        "serial feature disabled at build time",
    ));

    match opts.conn {
        ConnPref::Ble => {
            doctor_ble(&mut checks, addr, model, scan_secs, match_mode).await?;
        }
        ConnPref::Usb => {
            doctor_usb(&mut checks, addr, model).await?;
        }
    }

    // Support matrix tip
    let tested: Vec<_> = hardware_matrix()
        .iter()
        .filter(|h| h.status == SupportStatus::Tested)
        .map(|h| h.model)
        .collect();
    checks.push(Check::pass(
        "support_matrix",
        format!(
            "hardware-tested models: {} — run `thermark tasks`",
            tested.join(", ")
        ),
    ));

    Ok(DoctorReport { checks })
}

#[cfg(feature = "ble")]
async fn doctor_ble(
    checks: &mut Vec<Check>,
    addr: Option<&str>,
    model: Model,
    scan_secs: u64,
    match_mode: BleMatchMode,
) -> Result<()> {
    match transport::bluetooth_available().await {
        Ok(info) => checks.push(Check::pass("bluetooth", info)),
        Err(e) => {
            checks.push(Check::fail("bluetooth", format!("{e}")));
            return Ok(());
        }
    }

    let scan_for = Duration::from_secs(scan_secs.max(1));
    match BleTransport::scan(scan_for).await {
        Ok(devs) if devs.is_empty() => {
            checks.push(Check::fail(
                "ble_scan",
                format!("no printers in {scan_secs}s — power on, quit vendor apps, move closer"),
            ));
            #[cfg(all(target_os = "macos", feature = "serial"))]
            if let Some(selector) = addr
                && let Some(path) = transport::serial_port_for_selector(selector)
            {
                checks.push(Check::warn(
                    "ble_session",
                    format!(
                        "macOS exposes matching serial endpoint {path}; another Bluetooth client may own the printer. Disconnect it in Bluetooth settings and quit vendor apps before retrying BLE"
                    ),
                ));
            }
            if addr.is_none() {
                return Ok(());
            }
        }
        Ok(devs) => {
            let names: Vec<_> = devs.iter().map(|d| d.candidate.to_string()).collect();
            checks.push(Check::pass(
                "ble_scan",
                format!("{} device(s): {}", devs.len(), names.join("; ")),
            ));
        }
        Err(e) => {
            checks.push(Check::fail("ble_scan", format!("{e}")));
            return Ok(());
        }
    }

    if let Some(selector) = addr {
        match BleTransport::connect_with(selector, scan_for, match_mode).await {
            Ok(ble) => {
                checks.push(Check::pass(
                    "ble_connect",
                    format!("connected via '{selector}' ({match_mode:?})"),
                ));
                let mut client = PrinterClient::new(ble, model);
                match client.heartbeat().await {
                    Ok(hb) => {
                        checks.push(Check::pass("heartbeat", format!("{} bytes", hb.raw_len)));
                        checks.extend(evaluate_heartbeat(&hb));
                    }
                    Err(e) => checks.push(Check::fail("heartbeat", format!("{e}"))),
                }
                match client.rfid_info().await {
                    Ok(r) => checks.push(evaluate_rfid(&r)),
                    Err(e) => checks.push(Check::warn("rfid_tag", format!("query failed: {e}"))),
                }
                if let Ok(serial) = client
                    .get_info(crate::protocol::InfoKey::DeviceSerial)
                    .await
                {
                    checks.push(Check::pass("serial", serial.to_string()));
                }
                if let Err(e) = client.close().await {
                    checks.push(Check::warn("ble_disconnect", format!("{e}")));
                }
            }
            Err(e) => checks.push(Check::fail("ble_connect", format!("{e}"))),
        }
    } else {
        checks.push(Check::warn(
            "ble_connect",
            "skipped — pass -a \"PrinterName\" to test connect + sensors",
        ));
    }
    Ok(())
}

#[cfg(not(feature = "ble"))]
async fn doctor_ble(
    checks: &mut Vec<Check>,
    _addr: Option<&str>,
    _model: Model,
    _scan_secs: u64,
    _match_mode: BleMatchMode,
) -> Result<()> {
    checks.push(Check::fail(
        "bluetooth",
        "ble feature disabled at build time",
    ));
    Ok(())
}

#[cfg(feature = "serial")]
async fn doctor_usb(checks: &mut Vec<Check>, addr: Option<&str>, model: Model) -> Result<()> {
    let Some(path) = addr else {
        checks.push(Check::fail(
            "usb",
            "USB doctor requires -a /dev/cu.… (see thermark ports)",
        ));
        return Ok(());
    };
    match SerialTransport::open(path) {
        Ok(ser) => {
            checks.push(Check::pass("usb_open", path));
            let mut client = PrinterClient::new(ser, model);
            match client.heartbeat().await {
                Ok(hb) => {
                    checks.push(Check::pass("heartbeat", format!("{} bytes", hb.raw_len)));
                    checks.extend(evaluate_heartbeat(&hb));
                }
                Err(e) => checks.push(Check::fail("heartbeat", format!("{e}"))),
            }
        }
        Err(e) => checks.push(Check::fail("usb_open", format!("{e}"))),
    }
    Ok(())
}

#[cfg(not(feature = "serial"))]
async fn doctor_usb(checks: &mut Vec<Check>, _addr: Option<&str>, _model: Model) -> Result<()> {
    checks.push(Check::fail("usb", "serial feature disabled at build time"));
    Ok(())
}

/// Compatibility name for the connection preference used by [`DoctorOptions`].
///
/// New code should use [`ConnPref`] directly. Keeping this alias avoids breaking
/// callers while removing the duplicate enum and conversion layer.
pub type DoctorConn = ConnPref;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printer::Heartbeat;

    #[test]
    fn heartbeat_ready_all_pass() {
        let hb = Heartbeat {
            closing_state: Some(0),
            power_level: Some(3),
            paper_state: Some(0),
            rfid_read_state: Some(1),
            raw_len: 13,
        };
        let checks = evaluate_heartbeat(&hb);
        assert!(checks.iter().all(|c| c.status == CheckStatus::Pass));
    }

    #[test]
    fn heartbeat_open_cover_fails() {
        let hb = Heartbeat {
            closing_state: Some(1),
            power_level: Some(3),
            paper_state: Some(0),
            rfid_read_state: Some(1),
            raw_len: 13,
        };
        let checks = evaluate_heartbeat(&hb);
        let cover = checks.iter().find(|c| c.name == "cover").unwrap();
        assert_eq!(cover.status, CheckStatus::Fail);
    }

    #[test]
    fn heartbeat_no_paper_fails() {
        let hb = Heartbeat {
            closing_state: Some(0),
            power_level: Some(2),
            paper_state: Some(1),
            rfid_read_state: Some(1),
            raw_len: 13,
        };
        let checks = evaluate_heartbeat(&hb);
        assert!(
            checks
                .iter()
                .any(|c| c.name == "paper" && c.status == CheckStatus::Fail)
        );
    }

    #[test]
    fn overall_fail_dominates() {
        let r = DoctorReport {
            checks: vec![
                Check::pass("a", String::new()),
                Check::fail("b", String::new()),
            ],
        };
        assert_eq!(r.overall(), CheckStatus::Fail);
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn print_task_b1_is_pass() {
        let c = evaluate_print_task(Model::B1, None);
        assert_eq!(c.status, CheckStatus::Pass);
    }

    #[test]
    fn b1_task_does_not_mark_an_experimental_profile_as_tested() {
        let c = evaluate_print_task(Model::B21Pro, Some(PrintTask::B1));
        assert_eq!(c.status, CheckStatus::Warn);
        assert!(c.detail.contains("experimental"));
    }
}
