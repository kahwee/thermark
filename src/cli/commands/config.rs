//! `thermark config` — show / set / clear the saved default printer.

use anyhow::Result;
use thermark::config::{Config, ConnPref};
use thermark::protocol::Model;

use crate::cli::args::ConfigCmd;

pub fn run(action: ConfigCmd) -> Result<()> {
    match action {
        ConfigCmd::Path => println!("{}", Config::default_path()?.display()),
        ConfigCmd::Show { json } => show(json)?,
        ConfigCmd::Set {
            addr,
            conn,
            model,
            scan_secs,
        } => set(&addr, conn, model, scan_secs)?,
        ConfigCmd::SafeArea {
            top,
            bottom,
            left,
            right,
            reset,
        } => safe_area(top, bottom, left, right, reset)?,
        ConfigCmd::Clear => clear()?,
    }
    Ok(())
}

fn show(json: bool) -> Result<()> {
    let path = Config::default_path()?;
    let cfg = Config::load()?;
    if json {
        println!("{}", cfg.to_json_pretty()?);
        return Ok(());
    }
    println!("path: {}", path.display());
    if cfg.is_empty() && !path.exists() {
        println!("(no config file yet)");
        println!("Save a printer: thermark scan --save");
        println!("  or:            thermark config set -a \"B1-YourPrinter\" -m b1");
        return Ok(());
    }
    // field, value, what applies when unset
    let rows: [(&str, Option<String>, &str); 5] = [
        ("addr", cfg.addr.clone(), "(unset)"),
        (
            "connection",
            cfg.connection.map(|c| c.to_string()),
            "(default ble)",
        ),
        ("model", cfg.model.map(|m| m.to_string()), "(default b1)"),
        (
            "scan_secs",
            cfg.scan_secs.map(|n| n.to_string()),
            "(default 4)",
        ),
        (
            "safe_area",
            cfg.safe_area.map(|s| {
                format!(
                    "top {} / bottom {} / left {} / right {} px",
                    s.top, s.bottom, s.left, s.right
                )
            }),
            "(measured default)",
        ),
    ];
    for (name, value, fallback) in rows {
        println!("{name:<12} {}", value.as_deref().unwrap_or(fallback));
    }
    Ok(())
}

fn set(addr: &str, conn: ConnPref, model: Option<Model>, scan_secs: Option<u64>) -> Result<()> {
    let mut cfg = Config::load().unwrap_or_default();
    cfg.apply_set(addr, conn, model, scan_secs);
    let path = cfg.save()?;
    println!("saved default printer → {addr}");
    println!("  connection: {conn}");
    println!("  file:       {}", path.display());
    println!("Now you can run: thermark info   (no -a needed)");
    println!("JSON view:       thermark config show --json");
    Ok(())
}

/// Update the saved printable insets. Millimetres in, pixels stored.
fn safe_area(
    top: Option<f64>,
    bottom: Option<f64>,
    left: Option<f64>,
    right: Option<f64>,
    reset: bool,
) -> Result<()> {
    use thermark::geometry::{PX_PER_MM, SafeArea};

    let mut cfg = Config::load().unwrap_or_default();
    if reset {
        cfg.safe_area = None;
        cfg.save()?;
        println!("safe area reset to the built-in default");
        return Ok(());
    }

    let current = cfg.resolve_safe_area();
    let px = |mm: Option<f64>, fallback: u32| {
        mm.map(|v| (v.max(0.0) * PX_PER_MM).round() as u32)
            .unwrap_or(fallback)
    };
    let updated = SafeArea {
        top: px(top, current.top),
        bottom: px(bottom, current.bottom),
        left: px(left, current.left),
        right: px(right, current.right),
    };
    cfg.safe_area = Some(updated);
    let path = cfg.save()?;
    let mm = |v: u32| v as f64 / PX_PER_MM;
    println!(
        "safe area: top {:.1}mm  bottom {:.1}mm  left {:.1}mm  right {:.1}mm",
        mm(updated.top),
        mm(updated.bottom),
        mm(updated.left),
        mm(updated.right)
    );
    println!(
        "  ({} / {} / {} / {} px)",
        updated.top, updated.bottom, updated.left, updated.right
    );
    println!("  file: {}", path.display());
    println!("Confirm with: thermark calibrate --label 50x30");
    Ok(())
}

fn clear() -> Result<()> {
    let path = Config::default_path()?;
    if Config::clear()? {
        println!("removed {}", path.display());
    } else {
        println!("no config file at {}", path.display());
    }
    Ok(())
}
