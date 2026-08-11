//! `thermark doctor` — host + printer readiness.

use anyhow::{Context, Result};
use thermark::config::{Config, ConnPref};
use thermark::doctor::{self, DoctorOptions};
use thermark::print_task::PrintTask;
use thermark::protocol::Model;
use thermark::transport::BleMatchMode;

pub struct DoctorCommand {
    pub addr: Option<String>,
    pub conn: Option<ConnPref>,
    pub model: Option<Model>,
    pub task: Option<PrintTask>,
    pub seconds: u64,
    pub use_config: bool,
    pub fuzzy: bool,
}

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
        conn: cfg.resolve_connection(conn).into(),
        match_mode: BleMatchMode::from_fuzzy(fuzzy),
    })
    .await
    .context("doctor")?;

    print!("{report}");
    Ok(report.exit_code())
}
