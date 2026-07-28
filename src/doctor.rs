//! Environment + printer readiness checks (`thermark doctor`).

use crate::errors::Result;
use crate::print_task::{hardware_matrix, PrintTask, SupportStatus};
use crate::printer::{Heartbeat, PrinterClient, RfidInfo};
use crate::protocol::Model;
use std::fmt;
use std::time::Duration;

#[cfg(feature = "ble")]
use crate::transport::{self, BleTransport};
#[cfg(feature = "serial")]
use crate::transport::SerialTransport;

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
    let mut out = Vec::new();

    match hb.closing_state {
        Some(0) => out.push(Check {
            name: "cover".into(),
            status: CheckStatus::Pass,
            detail: "closed (0)".into(),
        }),
        Some(1) => out.push(Check {
            name: "cover".into(),
            status: CheckStatus::Fail,
            detail: "open (1) — close the lid fully".into(),
        }),
        Some(v) => out.push(Check {
            name: "cover".into(),
            status: CheckStatus::Warn,
            detail: format!("unexpected lid state {v}"),
        }),
        None => out.push(Check {
            name: "cover".into(),
            status: CheckStatus::Warn,
            detail: "not reported in heartbeat".into(),
        }),
    }

    match hb.paper_state {
        Some(0) => out.push(Check {
            name: "paper".into(),
            status: CheckStatus::Pass,
            detail: "detected (0)".into(),
        }),
        Some(1) => out.push(Check {
            name: "paper".into(),
            status: CheckStatus::Fail,
            detail: "not detected (1) — load labels, 2–5mm out".into(),
        }),
        Some(v) => out.push(Check {
            name: "paper".into(),
            status: CheckStatus::Warn,
            detail: format!("unexpected paper state {v}"),
        }),
        None => out.push(Check {
            name: "paper".into(),
            status: CheckStatus::Warn,
            detail: "not reported in heartbeat".into(),
        }),
    }

    match hb.rfid_read_state {
        Some(1) => out.push(Check {
            name: "rfid".into(),
            status: CheckStatus::Pass,
            detail: "ok (1)".into(),
        }),
        Some(0) => out.push(Check {
            name: "rfid".into(),
            status: CheckStatus::Warn,
            detail: "not ok (0) — third-party media may print poorly".into(),
        }),
        Some(v) => out.push(Check {
            name: "rfid".into(),
            status: CheckStatus::Warn,
            detail: format!("state {v}"),
        }),
        None => out.push(Check {
            name: "rfid".into(),
            status: CheckStatus::Warn,
            detail: "not reported".into(),
        }),
    }

    match hb.power_level {
        Some(0) => out.push(Check {
            name: "battery".into(),
            status: CheckStatus::Fail,
            detail: "empty / critical (0)".into(),
        }),
        Some(1) => out.push(Check {
            name: "battery".into(),
            status: CheckStatus::Warn,
            detail: "low (1) — charge before long jobs".into(),
        }),
        Some(v) => out.push(Check {
            name: "battery".into(),
            status: CheckStatus::Pass,
            detail: format!("level {v} (scale ~0–4)"),
        }),
        None => out.push(Check {
            name: "battery".into(),
            status: CheckStatus::Warn,
            detail: "not reported".into(),
        }),
    }

    out
}

pub fn evaluate_rfid(r: &RfidInfo) -> Check {
    if !r.tag_present {
        return Check {
            name: "rfid_tag".into(),
            status: CheckStatus::Warn,
            detail: "no RFID tag data (ok for some media)".into(),
        };
    }
    Check {
        name: "rfid_tag".into(),
        status: CheckStatus::Pass,
        detail: format!(
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
    }
}

pub fn evaluate_print_task(model: Model) -> Check {
    let task = PrintTask::for_model(model);
    let status = if task.hardware_tested() {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn
    };
    Check {
        name: "print_task".into(),
        status,
        detail: format!(
            "model={model} → task={task} ({})",
            if task.hardware_tested() {
                "hardware-tested"
            } else {
                "experimental"
            }
        ),
    }
}

/// Run diagnostic checks. `addr` optional: without it, only host + scan.
pub async fn run_doctor(
    addr: Option<&str>,
    model: Model,
    scan_secs: u64,
    conn_kind: DoctorConn,
) -> Result<DoctorReport> {
    let mut checks = Vec::new();

    // Build / crate
    checks.push(Check {
        name: "thermark".into(),
        status: CheckStatus::Pass,
        detail: format!("v{}", env!("CARGO_PKG_VERSION")),
    });

    // Print task honesty
    checks.push(evaluate_print_task(model));

    // Fonts
    let fonts = crate::font::list_available_fonts();
    if fonts.is_empty() {
        checks.push(Check {
            name: "fonts".into(),
            status: CheckStatus::Warn,
            detail: "no system TTF candidates found — pass --font".into(),
        });
    } else {
        checks.push(Check {
            name: "fonts".into(),
            status: CheckStatus::Pass,
            detail: format!("{} candidate(s), e.g. {}", fonts.len(), fonts[0].display()),
        });
    }

    // Serial ports (informational)
    #[cfg(feature = "serial")]
    match SerialTransport::list_ports() {
        Ok(ports) if ports.is_empty() => checks.push(Check {
            name: "serial_ports".into(),
            status: CheckStatus::Warn,
            detail: "none listed (BLE-only is fine)".into(),
        }),
        Ok(ports) => checks.push(Check {
            name: "serial_ports".into(),
            status: CheckStatus::Pass,
            detail: format!("{} port(s): {}", ports.len(), ports.join(", ")),
        }),
        Err(e) => checks.push(Check {
            name: "serial_ports".into(),
            status: CheckStatus::Warn,
            detail: format!("could not list: {e}"),
        }),
    }
    #[cfg(not(feature = "serial"))]
    checks.push(Check {
        name: "serial_ports".into(),
        status: CheckStatus::Warn,
        detail: "serial feature disabled at build time".into(),
    });

    match conn_kind {
        DoctorConn::Ble => {
            doctor_ble(&mut checks, addr, model, scan_secs).await?;
        }
        DoctorConn::Usb => {
            doctor_usb(&mut checks, addr, model).await?;
        }
    }

    // Support matrix tip
    let tested: Vec<_> = hardware_matrix()
        .iter()
        .filter(|h| h.status == SupportStatus::Tested)
        .map(|h| h.model)
        .collect();
    checks.push(Check {
        name: "support_matrix".into(),
        status: CheckStatus::Pass,
        detail: format!(
            "hardware-tested models: {} — run `thermark tasks`",
            tested.join(", ")
        ),
    });

    Ok(DoctorReport { checks })
}

#[cfg(feature = "ble")]
async fn doctor_ble(
    checks: &mut Vec<Check>,
    addr: Option<&str>,
    model: Model,
    scan_secs: u64,
) -> Result<()> {
    match transport::bluetooth_available().await {
        Ok(info) => checks.push(Check {
            name: "bluetooth".into(),
            status: CheckStatus::Pass,
            detail: info,
        }),
        Err(e) => {
            checks.push(Check {
                name: "bluetooth".into(),
                status: CheckStatus::Fail,
                detail: format!("{e}"),
            });
            return Ok(());
        }
    }

    let scan_for = Duration::from_secs(scan_secs.max(1));
    match BleTransport::scan(scan_for).await {
        Ok(devs) if devs.is_empty() => {
            checks.push(Check {
                name: "ble_scan".into(),
                status: CheckStatus::Fail,
                detail: format!(
                    "no printers in {scan_secs}s — power on, quit vendor apps, move closer"
                ),
            });
            if addr.is_none() {
                return Ok(());
            }
        }
        Ok(devs) => {
            let names: Vec<_> = devs
                .iter()
                .map(|d| format!("{} ({})", d.name, d.id))
                .collect();
            checks.push(Check {
                name: "ble_scan".into(),
                status: CheckStatus::Pass,
                detail: format!("{} device(s): {}", devs.len(), names.join("; ")),
            });
        }
        Err(e) => {
            checks.push(Check {
                name: "ble_scan".into(),
                status: CheckStatus::Fail,
                detail: format!("{e}"),
            });
            return Ok(());
        }
    }

    if let Some(selector) = addr {
        match BleTransport::connect(selector, scan_for).await {
            Ok(ble) => {
                checks.push(Check {
                    name: "ble_connect".into(),
                    status: CheckStatus::Pass,
                    detail: format!("connected via '{selector}'"),
                });
                let mut client = PrinterClient::new(ble, model);
                match client.heartbeat().await {
                    Ok(hb) => {
                        checks.push(Check {
                            name: "heartbeat".into(),
                            status: CheckStatus::Pass,
                            detail: format!("{} bytes", hb.raw_len),
                        });
                        checks.extend(evaluate_heartbeat(&hb));
                    }
                    Err(e) => checks.push(Check {
                        name: "heartbeat".into(),
                        status: CheckStatus::Fail,
                        detail: format!("{e}"),
                    }),
                }
                match client.rfid_info().await {
                    Ok(r) => checks.push(evaluate_rfid(&r)),
                    Err(e) => checks.push(Check {
                        name: "rfid_tag".into(),
                        status: CheckStatus::Warn,
                        detail: format!("query failed: {e}"),
                    }),
                }
                if let Ok(serial) = client
                    .get_info(crate::protocol::InfoKey::DeviceSerial)
                    .await
                {
                    checks.push(Check {
                        name: "serial".into(),
                        status: CheckStatus::Pass,
                        detail: serial.to_string(),
                    });
                }
                client.into_transport().disconnect().await.ok();
            }
            Err(e) => checks.push(Check {
                name: "ble_connect".into(),
                status: CheckStatus::Fail,
                detail: format!("{e}"),
            }),
        }
    } else {
        checks.push(Check {
            name: "ble_connect".into(),
            status: CheckStatus::Warn,
            detail: "skipped — pass -a \"PrinterName\" to test connect + sensors".into(),
        });
    }
    Ok(())
}

#[cfg(not(feature = "ble"))]
async fn doctor_ble(
    checks: &mut Vec<Check>,
    _addr: Option<&str>,
    _model: Model,
    _scan_secs: u64,
) -> Result<()> {
    checks.push(Check {
        name: "bluetooth".into(),
        status: CheckStatus::Fail,
        detail: "ble feature disabled at build time".into(),
    });
    Ok(())
}

#[cfg(feature = "serial")]
async fn doctor_usb(checks: &mut Vec<Check>, addr: Option<&str>, model: Model) -> Result<()> {
    let Some(path) = addr else {
        checks.push(Check {
            name: "usb".into(),
            status: CheckStatus::Fail,
            detail: "USB doctor requires -a /dev/cu.… (see thermark ports)".into(),
        });
        return Ok(());
    };
    match SerialTransport::open(path) {
        Ok(ser) => {
            checks.push(Check {
                name: "usb_open".into(),
                status: CheckStatus::Pass,
                detail: path.into(),
            });
            let mut client = PrinterClient::new(ser, model);
            match client.heartbeat().await {
                Ok(hb) => {
                    checks.push(Check {
                        name: "heartbeat".into(),
                        status: CheckStatus::Pass,
                        detail: format!("{} bytes", hb.raw_len),
                    });
                    checks.extend(evaluate_heartbeat(&hb));
                }
                Err(e) => checks.push(Check {
                    name: "heartbeat".into(),
                    status: CheckStatus::Fail,
                    detail: format!("{e}"),
                }),
            }
        }
        Err(e) => checks.push(Check {
            name: "usb_open".into(),
            status: CheckStatus::Fail,
            detail: format!("{e}"),
        }),
    }
    Ok(())
}

#[cfg(not(feature = "serial"))]
async fn doctor_usb(checks: &mut Vec<Check>, _addr: Option<&str>, _model: Model) -> Result<()> {
    checks.push(Check {
        name: "usb".into(),
        status: CheckStatus::Fail,
        detail: "serial feature disabled at build time".into(),
    });
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum DoctorConn {
    Ble,
    Usb,
}

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
        assert!(checks
            .iter()
            .any(|c| c.name == "paper" && c.status == CheckStatus::Fail));
    }

    #[test]
    fn overall_fail_dominates() {
        let r = DoctorReport {
            checks: vec![
                Check {
                    name: "a".into(),
                    status: CheckStatus::Pass,
                    detail: String::new(),
                },
                Check {
                    name: "b".into(),
                    status: CheckStatus::Fail,
                    detail: String::new(),
                },
            ],
        };
        assert_eq!(r.overall(), CheckStatus::Fail);
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn print_task_b1_is_pass() {
        let c = evaluate_print_task(Model::B1);
        assert_eq!(c.status, CheckStatus::Pass);
    }
}
