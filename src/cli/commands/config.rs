//! `thermark config` — show / set / clear the saved default printer.

use anyhow::{Result, ensure};
use thermark::config::{Config, ConnPref};
use thermark::protocol::Model;

use crate::cli::args::ConfigCmd;

pub fn run(action: &ConfigCmd) -> Result<()> {
    match action {
        ConfigCmd::Path => println!("{}", Config::default_path()?.display()),
        ConfigCmd::Show { json } => show(*json)?,
        ConfigCmd::Set {
            addr,
            conn,
            model,
            scan_secs,
            label,
        } => set(addr.as_deref(), *conn, *model, *scan_secs, label.as_deref())?,
        ConfigCmd::SafeArea {
            last_tick,
            label,
            top,
            bottom,
            left,
            right,
            reset,
        } => safe_area(SafeAreaUpdate {
            last_tick: *last_tick,
            label: label.clone(),
            top: *top,
            bottom: *bottom,
            left: *left,
            right: *right,
            reset: *reset,
        })?,
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
    let rows: [(&str, Option<String>, &str); 6] = [
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
        ("label", cfg.label.clone(), "(default 50x30)"),
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

fn set(
    addr: Option<&str>,
    conn: Option<ConnPref>,
    model: Option<Model>,
    scan_secs: Option<u64>,
    label: Option<&str>,
) -> Result<()> {
    ensure!(
        addr.is_some()
            || conn.is_some()
            || model.is_some()
            || scan_secs.is_some()
            || label.is_some(),
        "no config updates provided; pass --addr, --conn, --model, --scan-secs, or --label"
    );
    let mut cfg = Config::load()?;
    cfg.apply_update(addr, conn, model, scan_secs, label)?;
    let path = cfg.save()?;
    println!("saved config");
    if addr.is_some() {
        println!("  addr:       {}", cfg.addr.as_deref().unwrap_or("(unset)"));
    }
    if let Some(conn) = conn {
        println!("  connection: {conn}");
    }
    if let Some(model) = model {
        println!("  model:      {model}");
    }
    if let Some(scan_secs) = scan_secs {
        println!("  scan_secs:  {scan_secs}");
    }
    if label.is_some() {
        println!(
            "  label:      {}",
            cfg.label.as_deref().unwrap_or("(unset)")
        );
    }
    println!("  file:       {}", path.display());
    if cfg.addr.is_some() {
        println!("Now you can run: thermark info   (no -a needed)");
    }
    println!("JSON view:       thermark config show --json");
    Ok(())
}

/// Update saved content/registration insets. Millimetres in, pixels stored.
struct SafeAreaUpdate {
    last_tick: Option<f64>,
    label: Option<String>,
    top: Option<f64>,
    bottom: Option<f64>,
    left: Option<f64>,
    right: Option<f64>,
    reset: bool,
}

fn safe_area(update: SafeAreaUpdate) -> Result<()> {
    use anyhow::bail;
    use thermark::geometry::{LabelMm, MAX_DIMENSION_MM, SafeArea};

    let SafeAreaUpdate {
        last_tick,
        label,
        top,
        bottom,
        left,
        right,
        reset,
    } = update;

    let mut cfg = Config::load()?;
    if reset {
        cfg.safe_area = None;
        cfg.save()?;
        println!("safe area reset to the built-in default");
        return Ok(());
    }

    let validate_mm = |name: &str, value: Option<f64>| -> Result<Option<f64>> {
        if let Some(value) = value
            && (!value.is_finite() || !(0.0..=MAX_DIMENSION_MM).contains(&value))
        {
            bail!("{name} must be a finite value from 0 to {MAX_DIMENSION_MM} mm");
        }
        Ok(value)
    };
    let last_tick = validate_mm("last tick", last_tick)?;
    let top = validate_mm("top inset", top)?;
    let bottom = validate_mm("bottom inset", bottom)?;
    let left = validate_mm("left inset", left)?;
    let right = validate_mm("right inset", right)?;

    let label_mm = LabelMm::parse(&cfg.resolve_label(label.as_deref()))?;
    // A ruler reading is easier to report than an inset: the last tick that
    // printed tells us how much of the label the printer actually reaches.
    let bottom = match (last_tick, bottom) {
        (Some(tick), _) => {
            let height_mm = label_mm.height_mm;
            if tick > height_mm {
                bail!("last tick {tick} mm exceeds the label height {height_mm} mm");
            }
            let lost = (height_mm - tick).max(0.0);
            println!(
                "last tick {tick} mm on a {height_mm} mm label -> {lost} mm bottom content inset"
            );
            Some(lost)
        }
        (None, b) => b,
    };

    let profile = thermark::profile_for_model(cfg.resolve_model(None));
    let pixels_per_mm = profile.pixels_per_mm();
    let current = cfg.resolve_safe_area(pixels_per_mm);
    let px = |mm: Option<f64>, fallback: u32| {
        mm.map(|v| (v.max(0.0) * pixels_per_mm).round() as u32)
            .unwrap_or(fallback)
    };
    let updated = SafeArea {
        top: px(top, current.top),
        bottom: px(bottom, current.bottom),
        left: px(left, current.left),
        right: px(right, current.right),
    };
    let label_px = label_mm.to_pixels(profile.max_width_px, profile.pixels_per_mm());
    if updated.content(label_px).is_none() {
        bail!(
            "safe-area insets consume the entire {}x{} mm label",
            label_mm.width_mm,
            label_mm.height_mm
        );
    }
    cfg.safe_area = Some(updated);
    let path = cfg.save()?;
    let mm = |v: u32| v as f64 / pixels_per_mm;
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
