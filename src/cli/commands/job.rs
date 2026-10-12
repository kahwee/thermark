//! Prepared jobs never rerender CSVs, reload fonts, or apply rendering overrides.
use crate::cli::args::{JobCmd, JobPrintCommand};
use crate::cli::session::{
    Session, combine_job_and_close, ensure_target_print_allowed, resolve_target,
};
use anyhow::{Result, ensure};
use thermark::batch::JobReport;
use thermark::config::Config;
use thermark::prepared::PreparedJob;

pub async fn run(action: JobCmd) -> Result<()> {
    match action {
        JobCmd::Verify { job } => {
            let job = PreparedJob::open(&job)?;
            println!(
                "Verified {} pages; job {}",
                job.manifest().total,
                job.manifest().job_id
            );
        }
        JobCmd::Status { job, json } => {
            let job = PreparedJob::open(&job)?;
            let report = JobReport::read(&job)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Job {} ({} at {} DPI)",
                    report.job_id,
                    job.manifest().model,
                    job.manifest().dpi
                );
                for row in report.rows {
                    println!(
                        "Row {}: {}{}",
                        row.row,
                        match row.outcome {
                            Some(thermark::batch::Outcome::Confirmed) => "confirmed",
                            Some(_) => "uncertain",
                            None => "not sent",
                        },
                        row.stage
                            .map_or(String::new(), |stage| format!("; last stage {stage:?}"))
                    );
                }
                if report.incomplete_tail {
                    println!("Incomplete journal tail: inspect physical output before resuming.");
                }
            }
        }
        JobCmd::Print(args) => print(args, false).await?,
        JobCmd::Resume(args) => print(args, true).await?,
    }
    Ok(())
}
async fn print(args: JobPrintCommand, resume: bool) -> Result<()> {
    let job = PreparedJob::open(&args.job)?;
    let cfg = Config::load()?;
    let model =
        <thermark::protocol::Model as clap::ValueEnum>::from_str(&job.manifest().model, false)
            .map_err(anyhow::Error::msg)?;
    let media = thermark::geometry::LabelMm::parse(&cfg.resolve_label(args.label.as_deref()))?
        .to_pixels(
            thermark::profile_for_model(model).max_width_px,
            thermark::profile_for_model(model).pixels_per_mm(),
        );
    ensure!(
        (media.width_px, media.height_px) == (job.manifest().width_px, job.manifest().height_px),
        "declared media differs from prepared job; load matching media and pass --label WxH"
    );
    let mut task = args.task;
    if task.task.is_none() {
        task.task = job
            .manifest()
            .print_task
            .as_deref()
            .and_then(thermark::PrintTask::parse);
    }
    let target = resolve_target(&cfg, Some(model), &task)?;
    ensure_target_print_allowed(target, cfg.resolve_connection(args.conn.conn))?;
    let mut session = Session::connect(&args.conn.resolve(&cfg)?, target).await?;
    let result = thermark::batch::execute(
        &mut session,
        &job,
        args.start_at.unwrap_or(job.manifest().start_at),
        resume,
        args.acknowledge_uncertain,
        |row| println!("Confirmed label {row}/{}", job.manifest().total),
    )
    .await;
    let close = session.finish().await;
    combine_job_and_close(result, close)
}
