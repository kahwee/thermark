use super::*;
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

    // Without a connection only the requested/configured profile is available.
    // Connected paths add this check after applying the detected identity.
    if addr.is_none() {
        checks.push(evaluate_print_task(model, opts.task, opts.conn));
    } else {
        checks.push(Check::fail(
            "print_task",
            "not evaluated — connected printer identity unavailable",
        ));
    }

    checks.push(Check::pass(
        "fonts",
        "embedded DejaVu Sans available; use --font for other scripts",
    ));

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
            doctor_ble(&mut checks, addr, model, opts.task, scan_secs, match_mode).await?;
        }
        ConnPref::Usb => {
            doctor_usb(&mut checks, addr, model, opts.task).await?;
        }
    }

    // Support matrix tip
    let tested: Vec<_> = PROFILES
        .iter()
        .filter_map(|profile| {
            (profile.support_status == SupportStatus::Tested)
                .then_some(profile.tested_connection)
                .flatten()
                .map(|connection| format!("{} over {connection}", profile.display_name))
        })
        .collect();
    checks.push(Check::pass(
        "support_matrix",
        format!(
            "hardware-tested paths: {} — run `thermark tasks`",
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
    task: Option<PrintTask>,
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
                let mut client = diagnostic_client(ble, model, task);
                let print_task =
                    evaluate_connected_print_task(&mut client, task, ConnPref::Ble).await;
                replace_print_task_check(checks, print_task);
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
#[expect(
    clippy::unused_async,
    reason = "matches the async signature of the enabled transport diagnostic"
)]
async fn doctor_ble(
    checks: &mut Vec<Check>,
    _addr: Option<&str>,
    _model: Model,
    _task: Option<PrintTask>,
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
async fn doctor_usb(
    checks: &mut Vec<Check>,
    addr: Option<&str>,
    model: Model,
    task: Option<PrintTask>,
) -> Result<()> {
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
            let mut client = diagnostic_client(ser, model, task);
            let print_task = evaluate_connected_print_task(&mut client, task, ConnPref::Usb).await;
            replace_print_task_check(checks, print_task);
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
#[expect(
    clippy::unused_async,
    reason = "matches the async signature of the enabled transport diagnostic"
)]
async fn doctor_usb(
    checks: &mut Vec<Check>,
    _addr: Option<&str>,
    _model: Model,
    _task: Option<PrintTask>,
) -> Result<()> {
    checks.push(Check::fail("usb", "serial feature disabled at build time"));
    Ok(())
}
