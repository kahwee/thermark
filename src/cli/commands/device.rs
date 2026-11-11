//! Device discovery and inspection: `scan`, `ports`, `info`, `fonts`, `tasks`, `encode`.

use anyhow::{Context, Result};
#[cfg(feature = "ble")]
use std::time::Duration;
use thermark::PROFILES;
use thermark::config::Config;
#[cfg(feature = "ble")]
use thermark::config::ConnPref;
use thermark::font;
use thermark::terminal::escape;
#[cfg(feature = "serial")]
use thermark::transport::SerialTransport;
#[cfg(feature = "ble")]
use thermark::transport::{BleDeviceInfo, BleTransport};
#[cfg(feature = "ble")]
use tracing::info;

use crate::cli::args::ConnArgs;
use crate::cli::session::{PrintTarget, Session, TaskSelection};

#[derive(serde::Serialize)]
struct DensityReport {
    min: u8,
    max: u8,
    default: u8,
}

/// Presentation object shared by human and JSON identity output.
#[derive(serde::Serialize)]
struct PrinterReport<'a> {
    model: thermark::Model,
    model_id: u16,
    protocol_version: Option<u8>,
    firmware: Option<&'a str>,
    hardware: Option<&'a str>,
    dpi: u16,
    max_width_px: u32,
    print_direction: &'static str,
    density: DensityReport,
    task: Option<&'static str>,
    color_mode: &'static str,
}

impl<'a> PrinterReport<'a> {
    fn new(
        identity: &'a thermark::PrinterIdentity,
        profile: &'static thermark::PrinterProfile,
    ) -> Self {
        Self {
            model: profile.model,
            model_id: identity.model_id,
            protocol_version: identity.protocol_version,
            firmware: identity.firmware.as_deref(),
            hardware: identity.hardware.as_deref(),
            dpi: profile.dpi,
            max_width_px: profile.max_width_px,
            print_direction: profile.direction.as_str(),
            density: DensityReport {
                min: profile.density_min,
                max: profile.density_max,
                default: profile.density_default,
            },
            task: thermark::task_for_identity(identity).map(|value| value.as_str()),
            color_mode: "monochrome",
        }
    }
}

impl std::fmt::Display for PrinterReport<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "model: {}", self.model)?;
        writeln!(f, "model id: {}", self.model_id)?;
        writeln!(
            f,
            "protocol: {}",
            self.protocol_version
                .map_or_else(|| "unknown".into(), |value| value.to_string())
        )?;
        writeln!(
            f,
            "firmware: {}",
            escape(self.firmware.unwrap_or("unknown"))
        )?;
        writeln!(
            f,
            "hardware: {}",
            escape(self.hardware.unwrap_or("unknown"))
        )?;
        writeln!(f, "dpi: {}", self.dpi)?;
        writeln!(f, "max width: {} px", self.max_width_px)?;
        writeln!(f, "direction: {}", self.print_direction)?;
        writeln!(
            f,
            "density: {}..={} (default {})",
            self.density.min, self.density.max, self.density.default
        )?;
        writeln!(f, "task: {}", self.task.unwrap_or("unresolved"))?;
        writeln!(f, "color mode: {}", self.color_mode)
    }
}

#[cfg(feature = "ble")]
pub async fn scan(seconds: u64, save: bool, prefer_name: Option<&str>) -> Result<()> {
    info!(seconds, save, "scanning BLE");
    let devices = BleTransport::scan(Duration::from_secs(seconds)).await?;
    if devices.is_empty() {
        println!("No label-printer-like devices found.");
        println!("Tips: turn printer on, quit vendor apps, enable Bluetooth.");
        return Ok(());
    }
    println!("{:<40} {:<24} RSSI", "ID", "NAME");
    for d in &devices {
        let rssi = d.rssi.map(|r| r.to_string()).unwrap_or_else(|| "-".into());
        println!(
            "{:<40} {:<24} {}",
            escape(d.id()),
            escape(d.display_name()),
            rssi
        );
    }

    if !save {
        println!("\nSave default:  thermark scan --save");
        println!("  or:           thermark config set -a \"id:<platform ID>\"");
        println!(
            "Then:          thermark info   (saved platform ID; names are not authentication)"
        );
        return Ok(());
    }

    let pick = pick_scan_device(&devices, prefer_name)
        .ok_or_else(|| anyhow::anyhow!("--save needs one unique device; use --name with an exact name or ID from the scan (signal strength is not identity)"))?;
    // Persist the platform ID, never the unauthenticated advertising name.
    let addr = format!("id:{}", pick.id());
    let mut cfg = Config::load()?;
    cfg.apply_update(Some(&addr), Some(ConnPref::Ble), None, None, None)?;
    let path = cfg.save()?;
    println!("\nsaved default printer → {}", escape(&addr));
    println!("  file: {}", path.display());
    println!("Next: thermark doctor --use-config");
    println!("      thermark info");
    println!("      thermark calibrate --label 50x30");
    Ok(())
}

#[cfg(not(feature = "ble"))]
#[expect(
    clippy::unused_async,
    reason = "matches the async signature of Bluetooth scanning"
)]
pub async fn scan(_seconds: u64, _save: bool, _prefer_name: Option<&str>) -> Result<()> {
    anyhow::bail!("this thermark binary was built without Bluetooth support")
}

/// Save only an unambiguous selection; signal strength is not identity.
#[cfg(feature = "ble")]
pub fn pick_scan_device<'a>(
    devices: &'a [BleDeviceInfo],
    prefer_name: Option<&str>,
) -> Option<&'a BleDeviceInfo> {
    let mut matches = devices.iter().filter(|device| match prefer_name {
        Some(selector) => thermark::transport::score_ble_candidate(
            selector,
            &device.candidate,
            thermark::transport::BleMatchMode::Exact,
        )
        .is_some(),
        None => true,
    });
    let pick = matches.next()?;
    matches.next().is_none().then_some(pick)
}

#[cfg(feature = "serial")]
pub fn ports() -> Result<()> {
    let ports = SerialTransport::list_ports()?;
    if ports.is_empty() {
        println!("No serial ports found.");
    } else {
        for p in ports {
            println!("{p}");
        }
    }
    Ok(())
}

#[cfg(not(feature = "serial"))]
pub fn ports() -> Result<()> {
    anyhow::bail!("this thermark binary was built without USB serial support")
}

pub async fn info(cfg: &Config, conn: &ConnArgs) -> Result<()> {
    let conn = conn.resolve(cfg)?;
    let mut session = Session::connect(
        &conn,
        PrintTarget {
            model: thermark::Model::B1,
            task: TaskSelection::Auto {
                default: thermark::PrintTask::B1,
            },
            allow_experimental: true,
        },
    )
    .await?;
    let result = session.fetch_summary().await;
    let close_result = session.finish().await;
    let summary = result?;
    close_result?;
    print!("{summary}");
    Ok(())
}

pub async fn identify(cfg: &Config, conn: &ConnArgs, json: bool) -> Result<()> {
    let conn = conn.resolve(cfg)?;
    let session = Session::connect_detailed(
        &conn,
        PrintTarget {
            model: thermark::Model::B1,
            task: TaskSelection::Auto {
                default: thermark::PrintTask::B1,
            },
            allow_experimental: true,
        },
    )
    .await?;
    let result = (|| -> Result<()> {
        let identity = session
            .identity()
            .ok_or_else(|| anyhow::anyhow!("printer did not return an identity"))?;
        let profile = thermark::profile_for_identity(identity).ok_or_else(|| {
            anyhow::anyhow!(
                "unrecognized printer model id {}; add a checked device profile before printing",
                identity.model_id
            )
        })?;
        let report = PrinterReport::new(identity, profile);
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print!("{report}");
        }
        Ok(())
    })();
    let close = session.finish().await;
    result?;
    close
}

pub fn fonts() {
    println!(
        "Default: embedded DejaVu Sans (redistribution terms: LICENSE-FONTS). Named fonts below are host-dependent."
    );
    let fonts = font::list_available_fonts();
    if fonts.is_empty() {
        println!("No candidate fonts found.");
        return;
    }
    println!("Usable fonts on this machine:");
    for p in fonts {
        println!("  {}", p.display());
    }
    println!("\nUse with: thermark qr ... --font \"/path/to/font.ttf\"");
}

pub fn tasks() {
    println!(
        "{:<16} {:<10} {:<14} {:<12} NOTES",
        "MODEL", "TASK", "STATUS", "TESTED VIA"
    );
    for profile in PROFILES {
        println!(
            "{:<16} {:<10} {:<14} {:<12} {}",
            profile.display_name,
            profile.default_task.map_or("-", |task| task.as_str()),
            profile.support_status,
            profile.tested_connection.map_or("-", |conn| conn.as_str()),
            profile.support_notes
        );
    }
    println!();
    println!("Default: detected profile or b1. Override: --task b1|d11v1|d110|d110mv4");
    println!("Any path except B1+b1 over BLE requires: --allow-experimental");
}

pub fn encode(cmd: &str, data: &str) -> Result<()> {
    let cmd =
        u8::from_str_radix(cmd.trim_start_matches("0x"), 16).context("cmd must be hex, e.g. 1a")?;
    let data = if data.is_empty() {
        vec![]
    } else {
        hex::decode(data.trim_start_matches("0x")).context("data must be hex")?
    };
    let pkt = thermark::Packet::try_new(cmd, data).context("data too long for one packet")?;
    println!("{}", hex::encode(pkt.encode()?));
    Ok(())
}

#[cfg(test)]
mod report_tests {
    use super::*;

    #[test]
    fn human_identity_escapes_controls_without_changing_json_data() {
        let raw = "5.18\x1b]52;c;value\x07\u{202e}";
        let identity = thermark::PrinterIdentity {
            model_id: 4096,
            protocol_version: Some(5),
            firmware: Some(raw.into()),
            hardware: Some(raw.into()),
        };
        let report =
            PrinterReport::new(&identity, thermark::profile_for_model(thermark::Model::B1));
        assert!(!report.to_string().contains('\x1b'));
        assert!(!report.to_string().contains('\u{202e}'));
        assert_eq!(serde_json::to_value(&report).unwrap()["firmware"], raw);
    }

    #[test]
    fn b1_report_is_monochrome_and_machine_readable() {
        let identity = thermark::PrinterIdentity {
            model_id: 4096,
            protocol_version: Some(5),
            firmware: Some("5.18".into()),
            hardware: Some("5.10".into()),
        };
        let report =
            PrinterReport::new(&identity, thermark::profile_for_model(thermark::Model::B1));
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["model"], "b1");
        assert_eq!(json["model_id"], 4096);
        assert_eq!(json["max_width_px"], 384);
        assert_eq!(json["task"], "b1");
        assert_eq!(json["color_mode"], "monochrome");
        assert!(report.to_string().contains("firmware: 5.18"));
    }
}

#[cfg(all(test, feature = "ble"))]
mod tests {
    use super::*;
    use thermark::transport::BleCandidate;

    fn dev(name: Option<&str>, rssi: i16) -> BleDeviceInfo {
        BleDeviceInfo {
            candidate: BleCandidate::new(
                format!("id-{}", name.unwrap_or("anon")),
                name.map(String::from),
            ),
            rssi: Some(rssi),
        }
    }

    #[test]
    fn multiple_devices_need_an_exact_unique_selection() {
        let devices = vec![dev(Some("B1-One"), -50), dev(Some("B1-Two"), -40)];
        assert!(pick_scan_device(&devices, None).is_none());
        assert!(pick_scan_device(&devices, Some("two")).is_none());
        assert!(pick_scan_device(&devices, Some("missing")).is_none());
        assert_eq!(
            pick_scan_device(&devices, Some("B1-Two"))
                .unwrap()
                .display_name(),
            "B1-Two"
        );
    }
    #[test]
    fn duplicate_names_are_not_silently_ranked_by_signal() {
        let devices = vec![dev(Some("B1-One"), -50), dev(Some("B1-One"), -40)];
        assert!(pick_scan_device(&devices, Some("B1-One")).is_none());
    }
    #[test]
    fn singleton_and_empty_scans() {
        let devices = vec![dev(Some("B1-One"), -50)];
        assert!(pick_scan_device(&devices, None).is_some());
        assert!(pick_scan_device(&[], None).is_none());
    }
}
