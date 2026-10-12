//! CLI adapters for the reusable session lifecycle.
use super::args::{ConnArgs, TaskArgs};
use anyhow::Result;
use std::path::Path;
use thermark::config::Config;
use thermark::printer::PrintOptions;
use thermark::profile::PrinterProfile;
use thermark::protocol::Model;
pub use thermark::session::{
    PrintTarget, Session, TaskSelection, combine_job_and_close, ensure_target_print_allowed,
};
use thermark::session::{TargetOptions, run_file_session, run_rendered_session};
pub fn resolve_target(cfg: &Config, model: Option<Model>, args: &TaskArgs) -> Result<PrintTarget> {
    thermark::session::resolve_target(
        cfg,
        model,
        &TargetOptions {
            task: args.task,
            allow_experimental: args.allow_experimental,
        },
    )
}
/// Connect, print one file, disconnect — the sequence every printing command runs.
///
/// Disconnect happens before the print result is propagated, so a failed job
/// still releases the BLE link (only one client may hold it at a time).
pub async fn print_file_resolved(
    cfg: &Config,
    conn: &ConnArgs,
    target: PrintTarget,
    path: &Path,
    opts: PrintOptions,
    full_bleed: bool,
) -> Result<()> {
    ensure_target_print_allowed(target, cfg.resolve_connection(conn.conn))?;
    let conn = conn.resolve(cfg)?;
    let session = Session::connect(&conn, target).await?;
    run_file_session(session, cfg, path, opts, full_bleed).await
}

pub async fn render_and_print_gray_resolved<R>(
    cfg: &Config,
    conn: &ConnArgs,
    target: PrintTarget,
    density: thermark::Density,
    render: impl FnOnce(&'static PrinterProfile) -> Result<(image::GrayImage, R)>,
) -> Result<R> {
    ensure_target_print_allowed(target, cfg.resolve_connection(conn.conn))?;
    let resolved_conn = conn.resolve(cfg)?;
    let session = Session::connect(&resolved_conn, target).await?;
    run_rendered_session(session, density, render).await
}
