use super::*;
use crate::{MockTransport, Packet, protocol::Cmd};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Observations {
    starts: usize,
    closes: usize,
    packets: Vec<Packet>,
}

struct Observed {
    inner: MockTransport,
    observations: Arc<Mutex<Observations>>,
    fail_on_start: Option<usize>,
    hang: bool,
    waiting: bool,
    dir: std::path::PathBuf,
}

impl Transport for Observed {
    async fn send_raw(&mut self, bytes: &[u8]) -> crate::Result<()> {
        let packet = Packet::decode(bytes).unwrap();
        if packet.cmd == Cmd::PrintStart as u8 {
            let mut observations = self.observations.lock().unwrap();
            observations.starts += 1;
            // All pages exist and the uncertain marker is on disk before PrintStart.
            assert!(self.dir.join("label-00003.png").exists());
            let job = PreparedJob::open(&self.dir).unwrap();
            let report = JobReport::read(&job).unwrap();
            assert!(
                report
                    .rows
                    .iter()
                    .any(|r| r.outcome == Some(Outcome::Uncertain)
                        && r.stage == Some(crate::printer::PrintStage::Configuring))
            );
            if self.fail_on_start == Some(observations.starts) {
                self.inner.fail_cmd(Cmd::PrintStart as u8, 2);
            }
            self.waiting = self.hang;
        }
        self.observations.lock().unwrap().packets.push(packet);
        self.inner.send_raw(bytes).await
    }

    async fn recv_raw(&mut self, wait: Duration) -> crate::Result<Vec<u8>> {
        if self.waiting {
            std::future::pending().await
        } else {
            self.inner.recv_raw(wait).await
        }
    }

    async fn close(&mut self) -> crate::Result<()> {
        self.observations.lock().unwrap().closes += 1;
        self.inner.close().await
    }
}

fn command(dir: &Path) -> BatchOptions {
    BatchOptions {
        csv: "unused.csv".into(),
        preview_dir: dir.to_owned(),
        font: FontSelection {
            path: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf")),
            ..Default::default()
        },
        ..Default::default()
    }
}
fn rows() -> Vec<Row> {
    ["ONE", "TWO", "THREE"]
        .into_iter()
        .map(|text| Row {
            text: text.into(),
            url: None,
        })
        .collect()
}

async fn session(
    dir: &Path,
    model_id: u16,
    fail_on_start: Option<usize>,
    hang: bool,
) -> (Session<Observed>, Arc<Mutex<Observations>>) {
    let observations = Arc::new(Mutex::new(Observations::default()));
    let mut inner = MockTransport::new();
    inner.set_model_id(model_id);
    let transport = Observed {
        inner,
        observations: Arc::clone(&observations),
        fail_on_start,
        hang,
        waiting: false,
        dir: dir.to_owned(),
    };
    (
        Session::from_transport(
            transport,
            crate::config::ConnPref::Ble,
            crate::session::PrintTarget {
                model: crate::protocol::Model::B1,
                task: crate::session::TaskSelection::Auto {
                    default: crate::PrintTask::B1,
                },
                allow_experimental: true,
            },
            crate::Pacing::INSTANT,
        )
        .await
        .unwrap(),
        observations,
    )
}

fn journal(dir: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(dir.join("journal.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|v| v["status"] != "stage")
        .map(|mut v| {
            if v["row"].is_number() {
                v.as_object_mut().unwrap().remove("job_id");
                v.as_object_mut().unwrap().remove("stage");
            }
            v
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn batch_stops_on_second_job_and_closes_with_durable_outcomes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("run");
    let args = command(&dir);
    let cfg = Config::default();
    let layout = load_layout(&cfg, &args).unwrap();
    let (session, observations) = session(&dir, 4096, Some(2), false).await;
    let error = run_connected(session, &cfg, &args, &rows(), &layout)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("1 labels confirmed"));
    assert!(error.to_string().contains("row 2 uncertain"));
    let events = observations.lock().unwrap();
    assert_eq!(events.starts, 2);
    assert_eq!(events.closes, 1);
    let outcomes = journal(&dir);
    assert_eq!(outcomes[1], json!({"row": 1, "status": "uncertain"}));
    assert_eq!(outcomes[2], json!({"row": 1, "status": "confirmed"}));
    assert_eq!(outcomes[3], json!({"row": 2, "status": "uncertain"}));
    assert_eq!(outcomes.len(), 4);
}

#[tokio::test(start_paused = true)]
async fn batch_uses_detected_geometry_and_resumes_only_selected_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("run");
    let mut args = command(&dir);
    args.start_at = 2;
    let cfg = Config::default();
    let layout = load_layout(&cfg, &args).unwrap();
    let (session, observations) = session(&dir, 4097, None, false).await; // B1 Pro, 300 dpi
    run_connected(session, &cfg, &args, &rows(), &layout)
        .await
        .unwrap();
    assert_eq!(
        image::open(dir.join("label-00001.png"))
            .unwrap()
            .to_luma8()
            .dimensions(),
        (567, 354)
    );
    let events = observations.lock().unwrap();
    assert_eq!(events.starts, 2);
    assert_eq!(events.closes, 1);
    let pages: Vec<_> = events
        .packets
        .iter()
        .filter(|p| p.cmd == Cmd::SetPageSize as u8)
        .collect();
    assert_eq!(pages.len(), 2);
    for page in pages {
        assert_eq!(u16::from_be_bytes([page.data[0], page.data[1]]), 354);
        assert_eq!(u16::from_be_bytes([page.data[2], page.data[3]]), 567);
    }
    let outcomes = journal(&dir);
    assert_eq!(outcomes.len(), 5);
    assert_eq!(outcomes[1]["row"], 2);
    assert_eq!(outcomes[4], json!({"row": 3, "status": "confirmed"}));
}

#[tokio::test]
async fn late_render_error_and_save_error_send_no_jobs_and_close() {
    for save_error in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("run");
        let args = command(&dir);
        let cfg = Config::default();
        let layout = load_layout(&cfg, &args).unwrap();
        let mut input = rows();
        if save_error {
            fs::create_dir(&dir).unwrap();
        } else {
            input[2].text = String::new();
        }
        let (session, observations) = session(&dir, 4096, None, false).await;
        let error = run_connected(session, &cfg, &args, &input, &layout)
            .await
            .unwrap_err();
        if !save_error {
            assert!(error.to_string().contains("data row 3"));
            assert!(!dir.exists());
        }
        let events = observations.lock().unwrap();
        assert_eq!(events.starts, 0);
        assert_eq!(events.closes, 1);
        assert!(!dir.join("journal.jsonl").exists());
    }
}

#[tokio::test(start_paused = true)]
async fn interrupted_print_leaves_uncertain_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("run");
    let args = command(&dir);
    let cfg = Config::default();
    let layout = load_layout(&cfg, &args).unwrap();
    let (session, observations) = session(&dir, 4096, None, true).await;
    // Dropping the command future models main's Ctrl-C branch. Native transport
    // disconnect-on-drop is tested separately; this verifies the batch receipt.
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            run_connected(session, &cfg, &args, &rows(), &layout)
        )
        .await
        .is_err()
    );
    assert_eq!(observations.lock().unwrap().starts, 1);
    assert_eq!(
        journal(&dir).last().unwrap(),
        &json!({"row": 1, "status": "uncertain"})
    );
}

#[tokio::test]
async fn unrecognized_hardware_cannot_render_or_print_a_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("run");
    let args = command(&dir);
    let cfg = Config::default();
    let layout = load_layout(&cfg, &args).unwrap();
    let (session, observations) = session(&dir, 65535, None, false).await;
    assert!(
        run_connected(session, &cfg, &args, &rows(), &layout)
            .await
            .unwrap_err()
            .to_string()
            .contains("unrecognized")
    );
    assert!(!dir.exists());
    assert_eq!(observations.lock().unwrap().closes, 1);
}

fn prepare_demo(dir: &Path) -> PreparedJob {
    let args = command(dir);
    let cfg = Config::default();
    let layout = load_layout(&cfg, &args).unwrap();
    prepare_rows(
        &cfg,
        &args,
        &rows(),
        &layout,
        crate::profile_for_model(crate::protocol::Model::B1),
    )
    .unwrap()
}
#[test]
fn changed_pixels_or_settings_cannot_open_as_the_approved_job() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("job");
    let job = prepare_demo(&dir);
    let page = dir.join("label-00002.png");
    let original = fs::read(&page).unwrap();
    let mut image = job.page(2).unwrap();
    image.put_pixel(0, 0, image::Luma([0]));
    image.save(&page).unwrap();
    assert!(
        PreparedJob::open(&dir)
            .err()
            .unwrap()
            .to_string()
            .contains("hash mismatch")
    );
    fs::write(&page, original).unwrap();
    let path = dir.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["density"] = json!(5);
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        PreparedJob::open(&dir)
            .err()
            .unwrap()
            .to_string()
            .contains("manifest changed")
    );
}
#[test]
fn journal_is_locked_and_partial_confirmation_remains_uncertain() {
    let tmp = tempfile::tempdir().unwrap();
    let job = prepare_demo(&tmp.path().join("job"));
    let (mut journal, _) = Journal::create(&job, false, false).unwrap();
    assert!(Journal::create(&job, false, false).is_err());
    journal.record(Outcome::Uncertain, Some(1), None).unwrap();
    drop(journal);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(job.directory().join("journal.jsonl"))
        .unwrap();
    use std::io::Write;
    file.write_all(b"{\"status\":\"confirmed\"").unwrap();
    drop(file);
    let report = JobReport::read(&job).unwrap();
    assert!(report.incomplete_tail);
    assert_eq!(report.rows[0].outcome, Some(Outcome::Uncertain));
    assert!(Journal::create(&job, true, false).is_err());
    assert!(Journal::create(&job, true, true).is_ok());
}
#[tokio::test(start_paused = true)]
async fn resume_requires_review_and_never_reprints_confirmed_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("job");
    let job = prepare_demo(&dir);
    let (mut first, _) = session(&dir, 4096, Some(2), false).await;
    assert!(
        execute(&mut first, &job, 1, false, false, |_| {})
            .await
            .is_err()
    );
    first.finish().await.unwrap();
    let report = JobReport::read(&job).unwrap();
    assert_eq!(report.rows[0].outcome, Some(Outcome::Confirmed));
    assert_eq!(report.rows[1].outcome, Some(Outcome::Uncertain));
    assert!(report.rows[2].outcome.is_none());
    let (mut second, observations) = session(&dir, 4096, None, false).await;
    assert!(
        execute(&mut second, &job, 1, true, false, |_| {})
            .await
            .is_err()
    );
    assert_eq!(observations.lock().unwrap().starts, 0);
    execute(&mut second, &job, 1, true, true, |_| {})
        .await
        .unwrap();
    second.finish().await.unwrap();
    assert_eq!(observations.lock().unwrap().starts, 2);
    assert!(
        JobReport::read(&job)
            .unwrap()
            .rows
            .iter()
            .all(|r| r.outcome == Some(Outcome::Confirmed))
    );
}
#[tokio::test(start_paused = true)]
async fn prepared_job_rejects_a_different_detected_profile_before_printing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("job");
    let job = prepare_demo(&dir);
    let (mut connected, observations) = session(&dir, 4097, None, false).await;
    let error = execute(&mut connected, &job, 1, false, false, |_| {})
        .await
        .unwrap_err();
    assert!(error.to_string().contains("connected printer"));
    assert_eq!(observations.lock().unwrap().starts, 0);
    assert!(!dir.join("journal.jsonl").exists());
    connected.finish().await.unwrap();
}
#[tokio::test(start_paused = true)]
async fn command_failures_keep_uncertainty_and_never_send_the_next_label() {
    for command in [0x21, 0x23, 0x01, 0x03, 0x13, 0x85, 0xe3, 0xa3, 0xf3] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("job");
        let job = prepare_demo(&dir);
        let mut transport = crate::testing::FaultTransport::new(MockTransport::new());
        transport.fail_command = Some(command);
        transport.accepted_prefix = 3;
        let mut connected = Session::from_transport(
            transport,
            crate::config::ConnPref::Ble,
            crate::session::PrintTarget {
                model: crate::protocol::Model::B1,
                task: crate::session::TaskSelection::Auto {
                    default: crate::PrintTask::B1,
                },
                allow_experimental: false,
            },
            crate::Pacing::INSTANT,
        )
        .await
        .unwrap();
        assert!(
            execute(&mut connected, &job, 1, false, false, |_| {})
                .await
                .is_err(),
            "command {command:x}"
        );
        let report = JobReport::read(&job).unwrap();
        assert_eq!(report.rows[0].outcome, Some(Outcome::Uncertain));
        assert!(report.rows[1].outcome.is_none());
        assert!(report.rows[2].outcome.is_none());
        connected.finish().await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn connected_preparation_freezes_the_detected_firmware_task() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("job");
    let csv = tmp.path().join("labels.csv");
    fs::write(&csv, "text\nONE\n").unwrap();
    let args = BatchOptions {
        csv,
        preview_dir: dir.clone(),
        density: Some(Density::NORMAL),
        ..Default::default()
    };
    let mut transport = MockTransport::new();
    transport.set_model_id(512).set_protocol_version(1);
    let target = crate::session::PrintTarget {
        model: crate::protocol::Model::B1,
        task: crate::session::TaskSelection::Auto {
            default: crate::PrintTask::B1,
        },
        allow_experimental: true,
    };
    let session = Session::from_transport(
        transport,
        crate::config::ConnPref::Ble,
        target,
        crate::Pacing::INSTANT,
    )
    .await
    .unwrap();
    assert_eq!(session.print_task(), crate::PrintTask::D110);
    assert_eq!(
        session.profile().default_task,
        Some(crate::PrintTask::D11V1)
    );
    prepare_and_print(session, &Config::default(), &args)
        .await
        .unwrap();
    let job = PreparedJob::open(&dir).unwrap();
    assert_eq!(job.manifest().print_task.as_deref(), Some("d110"));
    assert_eq!(
        JobReport::read(&job).unwrap().rows[0].outcome,
        Some(Outcome::Confirmed)
    );
}
#[tokio::test(start_paused = true)]
async fn offline_preparation_rejects_a_different_firmware_task_before_sending() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("job");
    let csv = tmp.path().join("labels.csv");
    fs::write(&csv, "text\nONE\n").unwrap();
    let args = BatchOptions {
        csv,
        preview_dir: dir.clone(),
        density: Some(Density::NORMAL),
        ..Default::default()
    };
    let job = prepare(
        &Config::default(),
        &args,
        crate::profile_for_model(crate::protocol::Model::D11),
    )
    .unwrap();
    let mut transport = MockTransport::new();
    transport.set_model_id(512).set_protocol_version(1);
    let target = crate::session::PrintTarget {
        model: crate::protocol::Model::D11,
        task: crate::session::TaskSelection::Auto {
            default: crate::PrintTask::D11V1,
        },
        allow_experimental: true,
    };
    let mut session = Session::from_transport(
        transport,
        crate::config::ConnPref::Ble,
        target,
        crate::Pacing::INSTANT,
    )
    .await
    .unwrap();
    assert!(
        execute(&mut session, &job, 1, false, false, |_| {})
            .await
            .unwrap_err()
            .to_string()
            .contains("print task differs")
    );
    assert!(!dir.join("journal.jsonl").exists());
    session.finish().await.unwrap();
}
