//! Guided setup saves defaults only after identifying hardware and checking readiness.

use anyhow::{Context, Result, bail, ensure};
use std::io::{self, IsTerminal, Write};
use thermark::Density;
use thermark::config::{Config, ConnPref};
use thermark::geometry::LabelMm;
use thermark::label::TextSide;
use thermark::transport::BleMatchMode;

use crate::cli::args::{QrCommand, SetupCommand, TaskArgs};
use crate::cli::session::{PrintTarget, Session, TaskSelection, combine_job_and_close};

async fn prompt(message: String) -> Result<String> {
    ensure!(
        io::stdin().is_terminal(),
        "interactive setup needs a terminal; pass --addr and --label for unattended setup"
    );
    tokio::task::spawn_blocking(move || {
        print!("{message}");
        io::stdout().flush()?;
        let mut input = String::new();
        ensure!(io::stdin().read_line(&mut input)? > 0, "setup input closed");
        Ok(input.trim().to_owned())
    })
    .await
    .context("setup prompt")?
}

#[cfg(feature = "ble")]
async fn select_printer(seconds: u64) -> Result<String> {
    let devices =
        thermark::transport::BleTransport::scan(std::time::Duration::from_secs(seconds)).await?;
    ensure!(
        !devices.is_empty(),
        "no printers discovered; wake the printer, quit the vendor app, and check Bluetooth permission"
    );
    for (index, device) in devices.iter().enumerate() {
        println!(
            "  {}. {} ({})",
            index + 1,
            device.display_name(),
            device.id()
        );
    }
    let index = if devices.len() == 1 {
        0
    } else {
        let choice = prompt("Select printer number: ".into()).await?;
        let number: usize = choice.parse().context("enter a printer number")?;
        ensure!(
            (1..=devices.len()).contains(&number),
            "printer number is out of range"
        );
        number - 1
    };
    // IDs select the actual device even when multiple printers have the same name.
    Ok(devices[index].id().to_owned())
}

#[cfg(not(feature = "ble"))]
#[expect(clippy::unused_async, reason = "feature-independent setup dispatcher")]
async fn select_printer(_seconds: u64) -> Result<String> {
    bail!(
        "this thermark binary was built without Bluetooth support; use --conn usb --addr <serial path> with a serial-enabled build"
    )
}

pub async fn run(mut args: SetupCommand) -> Result<i32> {
    let mut cfg = Config::load()?;
    ensure!(
        !args.conn.fuzzy,
        "setup saves an exact printer selector; use the full name or ID from thermark scan instead of --fuzzy"
    );
    println!("Turn on and charge the printer, load your labels, and quit the vendor app.");
    let label = match args.label.take() {
        Some(label) => label,
        None => {
            let default = cfg.resolve_label(None);
            let input = prompt(format!("Physical label size in mm [{default}]: ")).await?;
            if input.is_empty() { default } else { input }
        }
    };
    LabelMm::parse(&label)?;
    let connection = cfg.resolve_connection(args.conn.conn);
    if args.conn.addr.is_none() && cfg.resolve_addr(None).is_err() {
        args.conn.addr = Some(match connection {
            ConnPref::Ble => select_printer(cfg.resolve_scan_secs(args.conn.scan_secs)).await?,
            ConnPref::Usb => prompt("Serial device path (see thermark ports): ".into()).await?,
        });
    }
    let conn = args.conn.resolve(&cfg)?;
    let session = Session::connect(
        &conn,
        PrintTarget {
            model: cfg.resolve_model(None),
            task: TaskSelection::Auto {
                default: thermark::PrintTask::B1,
            },
            allow_experimental: false,
        },
    )
    .await?;
    let identified = session
        .identity()
        .and_then(thermark::profile_for_identity)
        .ok_or_else(|| {
            anyhow::anyhow!("printer identity is unknown; setup did not change saved defaults")
        });
    let close = session.finish().await;
    let profile = combine_job_and_close(identified, close)?;
    let report = thermark::doctor::run_doctor(&thermark::doctor::DoctorOptions {
        addr: Some(conn.addr.clone()),
        model: profile.model,
        task: None,
        scan_secs: cfg.resolve_scan_secs(args.conn.scan_secs),
        conn: connection,
        match_mode: BleMatchMode::from_fuzzy(args.conn.fuzzy),
    })
    .await?;
    print!("{report}");
    if report.exit_code() != 0 {
        bail!(
            "readiness checks failed; setup did not change saved defaults. Resolve the checks above and retry"
        );
    }
    // Registration measurements belong to a specific printer. Keep them only
    // when setup is updating media on that same saved connection and model.
    if cfg.addr.as_deref() != Some(&conn.addr)
        || cfg.resolve_connection(None) != connection
        || cfg.resolve_model(None) != profile.model
    {
        if cfg.safe_area.is_some() {
            println!(
                "Printer changed; resetting saved registration insets. Recalibrate this printer if needed."
            );
        }
        cfg.safe_area = None;
    }
    cfg.apply_update(
        Some(&conn.addr),
        Some(connection),
        Some(profile.model),
        args.conn.scan_secs,
        Some(&label),
    )?;
    cfg.save()?;
    println!(
        "Saved {} over {connection}, media {label} mm.",
        profile.display_name
    );
    if args.test_print {
        super::sticker::qr(
            &cfg,
            QrCommand {
                conn: args.conn,
                task: TaskArgs {
                    task: None,
                    allow_experimental: false,
                },
                font: args.font,
                model: Some(profile.model),
                url: "https://example.com".into(),
                text: "Hello, label!".into(),
                text_side: TextSide::Right,
                label: Some(label),
                border: false,
                density: Density::DARK,
                save: None,
                no_print: false,
            },
        )
        .await?;
        println!(
            "Scan the printed QR: it should open example.com. Report whether it scans and how the text looks."
        );
    } else {
        println!(
            "Nothing printed. To print one test label, repeat with --label {label} --test-print."
        );
    }
    println!("Support report: thermark doctor --use-config --json");
    Ok(0)
}
