//! Environment + printer readiness checks (`thermark doctor`).

use crate::config::ConnPref;
use crate::errors::Result;
use crate::print_task::PrintTask;
use crate::printer::{Heartbeat, RfidInfo};
use crate::profile::{PROFILES, SupportStatus};
use crate::protocol::Model;
use serde::Serialize;
use std::fmt;

use crate::transport::BleMatchMode;
// Only a transport actually talks to a printer, so with both features off these
// are dead. Gated rather than `#[allow(unused)]` so a genuinely unused import
// still gets reported.
#[cfg(any(feature = "ble", feature = "serial", test))]
use crate::printer::PrinterClient;
#[cfg(any(feature = "ble", feature = "serial", test))]
use crate::transport::Transport;
#[cfg(feature = "ble")]
use std::time::Duration;

#[cfg(feature = "serial")]
use crate::transport::SerialTransport;
#[cfg(feature = "ble")]
use crate::transport::{self, BleTransport};

mod report;
use report::sensor_check;
pub use report::{Check, CheckStatus, DoctorReport};

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

/// Report the task and connection a print would actually use, honouring an
/// explicit `--task`.
pub fn evaluate_print_task(model: Model, task: Option<PrintTask>, connection: ConnPref) -> Check {
    let Some(task) = task.or_else(|| PrintTask::for_model(model)) else {
        return Check::warn(
            "print_task",
            format!("model={model} has no verified default task"),
        );
    };
    let profile = crate::profile::profile_for_model(model);
    let support = profile.print_path_support(task, connection);
    let (status, label) = match support {
        SupportStatus::Tested => (CheckStatus::Pass, "hardware-tested"),
        SupportStatus::Experimental => (CheckStatus::Warn, "experimental"),
        SupportStatus::Unresolved => (CheckStatus::Warn, "unresolved"),
    };
    Check::new(
        "print_task",
        status,
        format!(
            "model={model} → task={task} over {connection} ({label}), max width {}px",
            profile.max_width_px
        ),
    )
}

/// Build a read-only diagnostic client, including for profiles whose print
/// task is unresolved. The fallback task is inert: doctor never sends print
/// commands, and connected support is evaluated only after identification.
#[cfg(any(feature = "ble", feature = "serial", test))]
fn diagnostic_client<T: Transport>(
    transport: T,
    configured_model: Model,
    explicit_task: Option<PrintTask>,
) -> PrinterClient<T> {
    let provisional_task = explicit_task
        .or_else(|| PrintTask::for_model(configured_model))
        .unwrap_or(PrintTask::B1);
    PrinterClient::new_with_task(transport, configured_model, provisional_task)
}

/// Identify connected hardware before claiming that its print path is tested.
#[cfg(any(feature = "ble", feature = "serial", test))]
async fn evaluate_connected_print_task<T: Transport>(
    client: &mut PrinterClient<T>,
    explicit_task: Option<PrintTask>,
    connection: ConnPref,
) -> Check {
    let identity = match client.identify_profile().await {
        Ok(identity) => identity,
        Err(error) => {
            return Check::fail(
                "print_task",
                format!("not evaluated — printer identity query failed: {error}"),
            );
        }
    };
    let model_id = identity.model_id;
    let detected_task = explicit_task.or_else(|| crate::profile::task_for_identity(&identity));
    let Some(profile) = client.apply_identity(&identity, explicit_task.is_none()) else {
        return Check::fail(
            "print_task",
            format!("not evaluated — printer model id {model_id} is not in the profile registry"),
        );
    };

    let mut check = evaluate_print_task(profile.model, detected_task, connection);
    check.detail = format!("detected model id {model_id}: {}", check.detail);
    check
}

#[cfg(any(feature = "ble", feature = "serial", test))]
fn replace_print_task_check(checks: &mut Vec<Check>, connected: Check) {
    if let Some(check) = checks.iter_mut().find(|check| check.name == "print_task") {
        *check = connected;
    } else {
        checks.push(connected);
    }
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
mod probe;
pub use probe::run_doctor;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockTransport;
    use crate::printer::Heartbeat;

    #[test]
    fn support_json_keeps_results_and_omits_identifiers() {
        let report = DoctorReport {
            checks: vec![
                Check::pass("serial", "PRIVATE-SERIAL-123"),
                Check::pass("ble_connect", "connected via 'PRIVATE-PRINTER'"),
                Check::warn("battery", "PRIVATE-PATH-/Users/me/labels"),
                Check::fail("print_task", "identity query failed: PRIVATE-PRINTER"),
                Check::warn("PRIVATE-NAME", "PRIVATE-DETAIL"),
            ],
        };

        let body = report.support_json_pretty().unwrap();
        assert!(!body.contains("PRIVATE-SERIAL-123"));
        assert!(!body.contains("PRIVATE-PRINTER"));
        assert!(!body.contains("PRIVATE-PATH"));
        assert!(!body.contains("PRIVATE-NAME"));
        assert!(!body.contains("PRIVATE-DETAIL"));

        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["overall"], "fail");
        assert_eq!(
            json["privacy"],
            "printer identifiers and local paths omitted"
        );
        assert_eq!(json["checks"][0]["name"], "serial");
        assert!(json["checks"][0].get("detail").is_none());
        assert_eq!(json["checks"][2]["name"], "battery");
        assert!(json["checks"][2].get("detail").is_none());
        assert_eq!(json["checks"][3]["status"], "fail");
        assert_eq!(json["checks"][4]["name"], "other");
    }

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
        let c = evaluate_print_task(Model::B1, None, ConnPref::Ble);
        assert_eq!(c.status, CheckStatus::Pass);
    }

    #[test]
    fn b1_usb_path_is_not_reported_as_hardware_tested() {
        let c = evaluate_print_task(Model::B1, None, ConnPref::Usb);
        assert_eq!(c.status, CheckStatus::Warn);
        assert!(c.detail.contains("over usb"), "{}", c.detail);
        assert!(c.detail.contains("experimental"), "{}", c.detail);
    }

    #[test]
    fn b1_task_does_not_mark_an_experimental_profile_as_tested() {
        let c = evaluate_print_task(Model::B21Pro, Some(PrintTask::B1), ConnPref::Ble);
        assert_eq!(c.status, CheckStatus::Warn);
        assert!(c.detail.contains("experimental"));
    }

    #[tokio::test]
    async fn connected_task_support_uses_detected_profile_and_default_task() {
        let mut transport = MockTransport::new();
        transport.set_model_id(4097); // B1 Pro
        let mut client = diagnostic_client(transport, Model::B1, None);

        let check = evaluate_connected_print_task(&mut client, None, ConnPref::Ble).await;

        assert_eq!(check.name, "print_task");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("model=b1pro"), "{}", check.detail);
        assert!(check.detail.contains("task=d110mv4"), "{}", check.detail);
        assert!(check.detail.contains("experimental"), "{}", check.detail);
        assert!(
            !check.detail.contains("hardware-tested"),
            "{}",
            check.detail
        );
    }

    #[tokio::test]
    async fn configured_b18_doctor_is_unresolved_without_panicking_or_printing() {
        let mut transport = MockTransport::new();
        transport.set_model_id(3584); // B18
        let mut client = diagnostic_client(transport, Model::B18, None);

        let check = evaluate_connected_print_task(&mut client, None, ConnPref::Ble).await;

        assert_eq!(check.name, "print_task");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("model=b18"), "{}", check.detail);
        assert!(
            check.detail.contains("no verified default task"),
            "{}",
            check.detail
        );
        assert!(client.transport().tx_cmds().iter().all(|cmd| !matches!(
            *cmd,
            0x01 | 0x03 | 0x13 | 0x15 | 0x20 | 0x21 | 0x23 | 0x83..=0x85 | 0xe3 | 0xf3
        )));
    }

    #[tokio::test]
    async fn connected_doctor_preserves_explicit_task_for_detected_profile() {
        let mut transport = MockTransport::new();
        transport.set_model_id(4097); // B1 Pro
        let mut client = diagnostic_client(transport, Model::B1, Some(PrintTask::B1));

        let check =
            evaluate_connected_print_task(&mut client, Some(PrintTask::B1), ConnPref::Ble).await;

        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("model=b1pro"), "{}", check.detail);
        assert!(check.detail.contains("task=b1"), "{}", check.detail);
        assert_eq!(client.print_task(), PrintTask::B1);
    }

    #[tokio::test]
    async fn connected_unknown_model_cannot_inherit_configured_support() {
        let mut transport = MockTransport::new();
        transport.set_model_id(0xffff);
        let mut client = diagnostic_client(transport, Model::B1, None);

        let check = evaluate_connected_print_task(&mut client, None, ConnPref::Ble).await;

        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.detail.contains("model id 65535"), "{}", check.detail);
        assert!(
            !check.detail.contains("hardware-tested"),
            "{}",
            check.detail
        );
    }

    #[tokio::test]
    async fn failed_identity_query_cannot_inherit_configured_support() {
        let mut transport = MockTransport::new();
        transport.mute_cmd(crate::protocol::Cmd::PrinterInfo as u8);
        let mut client = diagnostic_client(transport, Model::B1, None)
            .with_pacing(crate::printer::Pacing::INSTANT);

        let check = evaluate_connected_print_task(&mut client, None, ConnPref::Ble).await;

        assert_eq!(check.status, CheckStatus::Fail);
        assert!(
            check.detail.contains("identity query failed"),
            "{}",
            check.detail
        );
        assert!(
            !check.detail.contains("hardware-tested"),
            "{}",
            check.detail
        );
    }

    #[test]
    fn connected_placeholder_preserves_the_print_task_check_surface() {
        let mut checks = vec![Check::fail(
            "print_task",
            "not evaluated — connected printer identity unavailable",
        )];
        replace_print_task_check(
            &mut checks,
            Check::warn("print_task", "detected experimental path"),
        );

        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, CheckStatus::Warn);
        assert_eq!(checks[0].detail, "detected experimental path");
    }
}
