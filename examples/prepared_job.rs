//! Prepare and execute a job with a mock transport; this never accesses hardware.
use thermark::batch::{BatchOptions, execute, prepare};
use thermark::session::{PrintTarget, Session, TaskSelection};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let work = tempfile::tempdir()?;
    let csv = work.path().join("demo.csv");
    std::fs::write(&csv, "text,url\nBIN A,https://example.com/a\n")?;
    let options = BatchOptions {
        csv,
        preview_dir: work.path().join("approved"),
        ..Default::default()
    };
    let job = prepare(
        &thermark::Config::default(),
        &options,
        thermark::profile_for_model(thermark::protocol::Model::B1),
    )?;
    let target = PrintTarget {
        model: thermark::protocol::Model::B1,
        task: TaskSelection::Auto {
            default: thermark::PrintTask::B1,
        },
        allow_experimental: false,
    };
    let mut session = Session::from_transport(
        thermark::MockTransport::new(),
        thermark::ConnPref::Ble,
        target,
        thermark::Pacing::INSTANT,
    )
    .await?;
    let result = execute(&mut session, &job, 1, false, false, |row| {
        println!("Mock protocol confirmed row {row}")
    })
    .await;
    let close = session.finish().await;
    thermark::session::combine_job_and_close(result, close)
}
