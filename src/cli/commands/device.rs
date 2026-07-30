//! Device discovery and inspection: `scan`, `ports`, `info`, `fonts`, `tasks`, `encode`.

use anyhow::{Context, Result};
use std::time::Duration;
use thermark::config::{Config, ConnPref};
use thermark::font;
use thermark::print_task::hardware_matrix;
use thermark::protocol::Model;
use thermark::transport::{BleDeviceInfo, BleTransport, SerialTransport};
use tracing::info;

use crate::cli::args::ConnArgs;
use crate::cli::session::Session;

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
    let mut cfg = Config::load().unwrap_or_default();
    let model = cfg.model;
    cfg.apply_set(&addr, ConnPref::Ble, model, None);
    let path = cfg.save()?;
    println!("\nsaved default printer → {addr}");
    println!("  file: {}", path.display());
    println!("Next: thermark doctor --use-config");
    println!("      thermark info");
    println!("      thermark calibrate --label 50x30");
    Ok(())
}

/// Choose a device for `--save`: name substring, else printer-like name, else strongest signal.
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

pub async fn info(cfg: &Config, conn: &ConnArgs, model: Option<Model>) -> Result<()> {
    let model = cfg.resolve_model(model);
    let conn = conn.resolve(cfg)?;
    // `info` never runs a print sequence, so the experimental gate does not apply.
    let mut session = Session::connect(&conn, model, thermark::print_task::PrintTask::B1).await?;
    let result = session.fetch_summary().await;
    session.finish().await;
    print!("{}", result?);
    Ok(())
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
            row.model, row.task, row.status, row.notes
        );
    }
    println!();
    println!("Default: b1 (hardware-tested). Override: --task b1|b21v1|d110|simple");
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
