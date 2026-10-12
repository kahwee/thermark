//! CLI adapter for library batch preparation and execution.
use crate::cli::args::BatchCommand;
use crate::cli::session::{Session, ensure_target_print_allowed, resolve_target};
use anyhow::Result;
use thermark::batch::{BatchOptions, FontSelection};
use thermark::config::Config;

pub fn options(args: &BatchCommand) -> BatchOptions {
    BatchOptions {
        csv: args.csv.clone(),
        template: args.template.clone(),
        preview_dir: args.preview_dir.clone(),
        start_at: args.start_at,
        font: FontSelection {
            path: args.font.font.clone(),
            name: args.font.font_name.clone(),
            size: args.font.font_size,
        },
        label: args.label.clone(),
        density: args.density,
        task: args.task.task,
    }
}
pub async fn run(cfg: &Config, args: BatchCommand) -> Result<()> {
    let options = options(&args);
    // Validate input before connecting, including malformed late CSV rows.
    let layout = thermark::batch::load_layout(cfg, &options)?;
    thermark::batch::load_rows(&options.csv, layout.kind())?;
    if !args.print {
        let job = thermark::batch::prepare(
            cfg,
            &options,
            thermark::profile_for_model(cfg.resolve_model(args.model)),
        )?;
        println!(
            "Validated {} labels; previews saved in {}\nJob {}\nNothing printed. Inspect the PNGs, then use `thermark job print --job {}` to print these exact pages.",
            job.manifest().total,
            job.directory().display(),
            job.manifest().job_id,
            job.directory().display()
        );
        return Ok(());
    }
    let target = resolve_target(cfg, args.model, &args.task)?;
    ensure_target_print_allowed(target, cfg.resolve_connection(args.conn.conn))?;
    let session = Session::connect(&args.conn.resolve(cfg)?, target).await?;
    thermark::batch::prepare_and_print(session, cfg, &options).await?;
    println!(
        "Batch completed; inspect journal.jsonl in {}",
        options.preview_dir.display()
    );
    Ok(())
}
