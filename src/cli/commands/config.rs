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
    let rows: [(&str, Option<String>, &str); 4] = [
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

fn clear() -> Result<()> {
    let path = Config::default_path()?;
    if Config::clear()? {
        println!("removed {}", path.display());
    } else {
        println!("no config file at {}", path.display());
    }
    Ok(())
}
