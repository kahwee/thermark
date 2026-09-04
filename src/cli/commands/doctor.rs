//! `thermark doctor` — host + printer readiness.

use anyhow::{Context, Result};
use thermark::config::Config;
use thermark::doctor::{self, DoctorOptions};
use thermark::transport::BleMatchMode;

use crate::cli::args::DoctorCommand;

pub async fn run(cfg: &Config, args: DoctorCommand) -> Result<i32> {
    let DoctorCommand {
        addr,
        conn,
        model,
        task,
        seconds,
        use_config,
        fuzzy,
    } = args;
    // Host-only by default; -a or --use-config enables connect + sensors.
    let addr = match addr {
        Some(a) => Some(a),
        None if use_config => Some(cfg.resolve_addr(None)?),
        None => None,
    };

    let report = doctor::run_doctor(&DoctorOptions {
        addr,
        model: cfg.resolve_model(model),
        task,
        scan_secs: seconds,
        conn: cfg.resolve_connection(conn),
        match_mode: BleMatchMode::from_fuzzy(fuzzy),
    })
    .await
    .context("doctor")?;

    print!("{report}");
    Ok(report.exit_code())
}
