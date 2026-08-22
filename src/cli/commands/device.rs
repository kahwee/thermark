//! Device discovery and inspection: `scan`, `ports`, `info`, `fonts`, `tasks`, `encode`.

use anyhow::{Context, Result};
#[cfg(feature = "ble")]
use std::time::Duration;
use thermark::config::Config;
#[cfg(feature = "ble")]
use thermark::config::ConnPref;
use thermark::font;
use thermark::print_task::hardware_matrix;
#[cfg(feature = "serial")]
use thermark::transport::SerialTransport;
#[cfg(feature = "ble")]
use thermark::transport::{BleDeviceInfo, BleTransport};
#[cfg(feature = "ble")]
use tracing::info;

use crate::cli::args::ConnArgs;
use crate::cli::session::Session;

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
        writeln!(f, "firmware: {}", self.firmware.unwrap_or("unknown"))?;
        writeln!(f, "hardware: {}", self.hardware.unwrap_or("unknown"))?;
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
        println!("{:<40} {:<24} {}", d.id(), d.display_name(), rssi);
    }

    if !save {
        println!("\nSave default:  thermark scan --save");
        println!("  or:           thermark config set -a \"<full advertising name>\"");
        println!("Then:          thermark info   (exact name; add --fuzzy only if needed)");
        return Ok(());
    }

    let pick = pick_scan_device(&devices, prefer_name)
        .ok_or_else(|| anyhow::anyhow!("no device matched for --save"))?;
    // Prefer the advertising name; fall back to the peripheral id (macOS UUID).
    let addr = pick
        .candidate
        .name
        .clone()
        .unwrap_or_else(|| pick.candidate.id.clone());
    let mut cfg = Config::load()?;
    let model = cfg.model;
    cfg.apply_set(&addr, ConnPref::Ble, model, None)?;
    let path = cfg.save()?;
    println!("\nsaved default printer → {addr}");
    println!("  file: {}", path.display());
    println!("Next: thermark doctor --use-config");
    println!("      thermark info");
    println!("      thermark calibrate --label 50x30");
    Ok(())
}

#[cfg(not(feature = "ble"))]
pub async fn scan(_seconds: u64, _save: bool, _prefer_name: Option<&str>) -> Result<()> {
    anyhow::bail!("this thermark binary was built without Bluetooth support")
}

/// Choose a device for `--save`: name substring, else printer-like name, else strongest signal.
#[cfg(feature = "ble")]
pub fn pick_scan_device<'a>(
    devices: &'a [BleDeviceInfo],
    prefer_name: Option<&str>,
) -> Option<&'a BleDeviceInfo> {
    if let Some(want) = prefer_name.map(|s| s.to_ascii_lowercase()) {
        let hit = devices.iter().find(|d| {
            d.display_name().to_ascii_lowercase().contains(&want)
                || d.id().to_ascii_lowercase().contains(&want)
        });
        if hit.is_some() {
            return hit;
        }
    }
    devices.iter().max_by_key(|d| {
        // Printer-like names outrank raw signal; unnamed devices rank last.
        let name_bonus = if d.candidate.looks_like_label_printer() {
            1000
        } else if d.candidate.name.is_none() {
            -200
        } else {
            0
        };
        name_bonus + i32::from(d.rssi.unwrap_or(-100))
    })
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
        thermark::Model::B1,
        thermark::PrintTask::B1,
        true,
        true,
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
    let session = Session::connect(
        &conn,
        thermark::Model::B1,
        thermark::PrintTask::B1,
        true,
        true,
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
    println!("{:<16} {:<10} {:<14} NOTES", "MODEL", "TASK", "STATUS");
    for row in hardware_matrix() {
        println!(
            "{:<16} {:<10} {:<14} {}",
            row.model,
            row.task.map_or("-", |task| task.as_str()),
            row.status,
            row.notes
        );
    }
    println!();
    println!("Default: detected profile or b1. Override: --task b1|d11v1|d110|d110mv4");
    println!("Non-b1 tasks require: --allow-experimental");
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
    fn prefers_printer_name_over_stronger_rssi() {
        let devices = vec![dev(Some("random-watch"), -40), dev(Some("B1-ABC"), -80)];
        assert_eq!(
            pick_scan_device(&devices, None).unwrap().display_name(),
            "B1-ABC"
        );
    }

    #[test]
    fn name_filter_wins() {
        let devices = vec![dev(Some("B1-One"), -50), dev(Some("B1-Two"), -40)];
        assert_eq!(
            pick_scan_device(&devices, Some("two"))
                .unwrap()
                .display_name(),
            "B1-Two"
        );
    }

    #[test]
    fn unnamed_device_ranks_last() {
        let devices = vec![dev(None, -30), dev(Some("B1-Far"), -90)];
        assert_eq!(
            pick_scan_device(&devices, None).unwrap().display_name(),
            "B1-Far"
        );
    }

    #[test]
    fn empty_scan_has_no_pick() {
        assert!(pick_scan_device(&[], None).is_none());
    }
}
