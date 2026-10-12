//! Typed, durable outcomes with an OS lock and conservative crash recovery.
use crate::prepared::{PreparedJob, read_bounded};
use crate::printer::PrintStage;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Prepared,
    Uncertain,
    Stage,
    Confirmed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub job_id: String,
    pub status: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<PrintStage>,
}
#[derive(Debug, Serialize)]
pub struct JobReport {
    pub job_id: String,
    pub rows: Vec<RowReport>,
    pub incomplete_tail: bool,
}
#[derive(Debug, Serialize)]
pub struct RowReport {
    pub row: u32,
    pub outcome: Option<Outcome>,
    pub stage: Option<PrintStage>,
}
impl JobReport {
    pub fn read(job: &PreparedJob) -> Result<Self> {
        let mut paths: Vec<_> = fs::read_dir(job.directory())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name().and_then(|s| s.to_str()).is_some_and(|n| {
                    n == "journal.jsonl" || (n.starts_with("journal-") && n.ends_with(".jsonl"))
                })
            })
            .collect();
        paths.sort_by_key(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            if n == "journal.jsonl" {
                "journal-00000.jsonl".to_string()
            } else {
                n.into_owned()
            }
        });
        let mut report = Self {
            job_id: job.manifest().job_id.clone(),
            rows: (1..=job.manifest().total as u32)
                .map(|row| RowReport {
                    row,
                    outcome: None,
                    stage: None,
                })
                .collect(),
            incomplete_tail: false,
        };
        for path in paths {
            let bytes = read_bounded(&path, 8 * 1024 * 1024)?;
            // Only complete newline-terminated entries are receipts.
            let end = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            if end != bytes.len() {
                report.incomplete_tail = true;
            }
            let mut active = None;
            for line in bytes[..end]
                .split(|b| *b == b'\n')
                .filter(|l| !l.is_empty())
            {
                let event: Event = serde_json::from_slice(line)
                    .with_context(|| format!("invalid journal {}", path.display()))?;
                ensure!(
                    event.job_id == report.job_id,
                    "journal belongs to a different job"
                );
                if event.status == Outcome::Prepared {
                    ensure!(
                        event.row.is_none() && event.stage.is_none() && active.is_none(),
                        "invalid prepared event"
                    );
                    continue;
                }
                let row = event.row.context("journal event needs row")?;
                let item = report
                    .rows
                    .get_mut(row.checked_sub(1).context("invalid journal row")? as usize)
                    .context("journal row outside job")?;
                match event.status {
                    Outcome::Uncertain => {
                        ensure!(active.is_none(), "previous journal row not completed");
                        active = Some(row);
                        item.outcome = Some(Outcome::Uncertain);
                        item.stage = None;
                    }
                    Outcome::Stage => {
                        ensure!(
                            active == Some(row) && event.stage.is_some(),
                            "stage without active row"
                        );
                        item.stage = event.stage;
                    }
                    Outcome::Confirmed => {
                        ensure!(active == Some(row), "confirmation without uncertain entry");
                        active = None;
                        item.outcome = Some(Outcome::Confirmed);
                    }
                    Outcome::Prepared => unreachable!(),
                }
            }
        }
        Ok(report)
    }
}

pub struct Journal {
    file: File,
    _lock: File,
    job_id: String,
}
impl Journal {
    /// Locks the job across processes. The OS releases the lock even after a crash.
    pub fn create(
        job: &PreparedJob,
        resume: bool,
        acknowledge_uncertain: bool,
    ) -> Result<(Self, JobReport)> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(job.directory().join("printing.lock"))?;
        lock.try_lock()
            .context("another process is printing this prepared job")?;
        let report = JobReport::read(job)?;
        let existing = report.rows.iter().any(|r| r.outcome.is_some());
        ensure!(
            resume || !existing,
            "job has prior outcomes; inspect `job status` and use `job resume`"
        );
        ensure!(
            !resume
                || acknowledge_uncertain
                || (!report.incomplete_tail
                    && report
                        .rows
                        .iter()
                        .all(|r| r.outcome != Some(Outcome::Uncertain))),
            "uncertain output or incomplete journal: inspect physical labels and pass --acknowledge-uncertain"
        );
        let first = job.directory().join("journal.jsonl");
        let mut index = 0u32;
        let file = loop {
            let path = if index == 0 {
                first.clone()
            } else {
                job.directory().join(format!("journal-{index:05}.jsonl"))
            };
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(file) => break file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    index += 1;
                    ensure!(index <= 1000, "too many job attempts");
                }
                Err(e) => return Err(e.into()),
            }
        };
        let mut journal = Self {
            file,
            _lock: lock,
            job_id: job.manifest().job_id.clone(),
        };
        journal.record(Outcome::Prepared, None, None)?;
        // Persist newly-created journal directory entry before hardware can act.
        #[cfg(unix)]
        File::open(job.directory())?.sync_all()?;
        Ok((journal, report))
    }
    pub fn record(
        &mut self,
        status: Outcome,
        row: Option<u32>,
        stage: Option<PrintStage>,
    ) -> Result<()> {
        serde_json::to_writer(
            &mut self.file,
            &Event {
                job_id: self.job_id.clone(),
                status,
                row,
                stage,
            },
        )?;
        self.file.write_all(b"\n")?;
        self.file.sync_all().context("sync batch journal")
    }
}
