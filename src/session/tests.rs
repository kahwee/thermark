use super::*;
use crate::geometry::{LabelMm, SafeArea};
use crate::printer::Pacing;
use crate::protocol::Cmd;
use anyhow::Context;
use image::{GrayImage, Luma};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Send { cmd: u8, data: Vec<u8> },
    Render,
    Close,
}

type Events = Arc<Mutex<Vec<Event>>>;

struct ObservedTransport {
    inner: crate::MockTransport,
    events: Events,
    close_failure: AtomicBool,
}

impl ObservedTransport {
    fn fail_close(&self) {
        self.close_failure.store(true, Ordering::Relaxed);
    }

    fn with_model_id(model_id: u16) -> (Self, Events) {
        let mut inner = crate::MockTransport::new();
        inner.set_model_id(model_id);
        Self::new(inner)
    }

    fn new(inner: crate::MockTransport) -> (Self, Events) {
        let events = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                inner,
                events: Arc::clone(&events),
                close_failure: AtomicBool::new(false),
            },
            events,
        )
    }
}

impl Transport for ObservedTransport {
    async fn send_raw(&mut self, data: &[u8]) -> crate::Result<()> {
        let packet = crate::Packet::decode(data).expect("client must send a valid packet");
        self.events.lock().unwrap().push(Event::Send {
            cmd: packet.cmd,
            data: packet.data,
        });
        self.inner.send_raw(data).await
    }

    async fn recv_raw(&mut self, wait: Duration) -> crate::Result<Vec<u8>> {
        self.inner.recv_raw(wait).await
    }

    async fn close(&mut self) -> crate::Result<()> {
        self.events.lock().unwrap().push(Event::Close);
        if self.close_failure.load(Ordering::Relaxed) {
            return Err(crate::Error::msg("synthetic disconnect failure"));
        }
        self.inner.close().await
    }
}

async fn detected_session(
    model_id: u16,
    auto_task: bool,
    allow_experimental: bool,
) -> (Session<ObservedTransport>, Events) {
    detected_session_over(model_id, ConnPref::Ble, auto_task, allow_experimental).await
}

async fn detected_session_over(
    model_id: u16,
    connection: ConnPref,
    auto_task: bool,
    allow_experimental: bool,
) -> (Session<ObservedTransport>, Events) {
    let (transport, events) = ObservedTransport::with_model_id(model_id);
    let client = PrinterClient::new_with_task(transport, Model::B1, PrintTask::B1)
        .with_pacing(Pacing::INSTANT);
    let session = Session::<ObservedTransport>::finish_connect(
        client,
        connection,
        PrintTarget {
            model: Model::B1,
            task: if auto_task {
                TaskSelection::Auto {
                    default: PrintTask::B1,
                }
            } else {
                TaskSelection::Explicit(PrintTask::B1)
            },
            allow_experimental,
        },
        IdentityDetail::Profile,
    )
    .await
    .unwrap();
    (session, events)
}

fn event_position(events: &[Event], predicate: impl Fn(&Event) -> bool) -> usize {
    events
        .iter()
        .position(predicate)
        .expect("expected lifecycle event")
}

fn first_packet(events: &[Event], command: Cmd) -> &[u8] {
    events
        .iter()
        .find_map(|event| match event {
            Event::Send { cmd, data } if *cmd == command as u8 => Some(data.as_slice()),
            _ => None,
        })
        .expect("expected command packet")
}

fn is_print_mutation(event: &Event) -> bool {
    matches!(
        event,
        Event::Send { cmd, .. }
            if matches!(
                *cmd,
                0x01 | 0x03 | 0x13 | 0x15 | 0x20 | 0x21 | 0x23
                    | 0x83..=0x86 | 0xe3 | 0xf3
            )
    )
}

fn args(task: Option<PrintTask>, allow: bool) -> TargetOptions {
    TargetOptions {
        task,
        allow_experimental: allow,
    }
}

#[test]
fn explicit_task_is_used() {
    let t = resolve_task(Model::B1, &args(Some(PrintTask::B1), false)).unwrap();
    assert_eq!(t, PrintTask::B1);
}

#[test]
fn experimental_task_resolution_is_safe_offline_without_opt_in() {
    assert_eq!(
        resolve_task(Model::B1, &args(Some(PrintTask::D110), false)).unwrap(),
        PrintTask::D110
    );
}

#[test]
fn model_default_is_used_without_flags() {
    assert_eq!(
        resolve_task(Model::B1, &args(None, false)).unwrap(),
        PrintTask::B1
    );
    assert_eq!(
        resolve_task(Model::D110, &args(None, false)).unwrap(),
        PrintTask::D110
    );
}

#[test]
fn experimental_task_is_gated_at_print_boundary() {
    let target = resolve_target(&Config::default(), Some(Model::D110), &args(None, false)).unwrap();
    assert!(ensure_target_print_allowed(target, ConnPref::Ble).is_err());

    let allowed = PrintTarget {
        allow_experimental: true,
        ..target
    };
    assert!(ensure_target_print_allowed(allowed, ConnPref::Ble).is_ok());
}

#[test]
fn detected_experimental_profile_with_explicit_b1_task_stays_gated() {
    let mut client =
        PrinterClient::new_with_task(crate::MockTransport::new(), Model::B1, PrintTask::B1);
    let identity = crate::PrinterIdentity {
        model_id: 785,
        protocol_version: Some(5),
        firmware: None,
        hardware: None,
    };

    // `false` models an explicit --task override: identification updates
    // the physical profile but intentionally preserves the selected task.
    client.apply_identity(&identity, false).unwrap();
    assert_eq!(client.model(), Model::B21Pro);
    assert_eq!(client.print_task(), PrintTask::B1);
    assert!(
        ensure_print_path_allowed(client.model(), client.print_task(), ConnPref::Ble, false)
            .is_err()
    );
    assert!(
        ensure_print_path_allowed(client.model(), client.print_task(), ConnPref::Ble, true).is_ok()
    );
}

#[test]
fn b1_usb_path_requires_explicit_opt_in() {
    assert!(ensure_print_path_allowed(Model::B1, PrintTask::B1, ConnPref::Usb, false).is_err());
    assert!(ensure_print_path_allowed(Model::B1, PrintTask::B1, ConnPref::Usb, true).is_ok());
    assert!(ensure_print_path_allowed(Model::B1, PrintTask::B1, ConnPref::Ble, false).is_ok());
}

#[tokio::test]
async fn detected_profile_drives_render_and_wire_geometry() {
    let cases = [
        (4096, Model::B1, PrintTask::B1, 203, 384, 240, 8),
        (4097, Model::B1Pro, PrintTask::D110MV4, 300, 567, 354, 12),
        (785, Model::B21Pro, PrintTask::D110MV4, 300, 591, 354, 12),
        (2304, Model::D110, PrintTask::D110, 203, 96, 240, 8),
    ];

    for (model_id, model, task, dpi, width, height, safe_top) in cases {
        let (session, events) = detected_session(model_id, true, true).await;
        assert_eq!(session.profile().model, model);
        assert_eq!(session.client.print_task(), task);

        let render_events = Arc::clone(&events);
        let rendered = run_rendered_session(session, crate::Density::NORMAL, |profile| {
            let label =
                LabelMm::parse("50x30")?.to_pixels(profile.max_width_px, profile.pixels_per_mm());
            let safe = Config::default().resolve_safe_area(profile.pixels_per_mm());
            render_events.lock().unwrap().push(Event::Render);
            Ok((
                GrayImage::from_pixel(label.width_px, label.height_px, Luma([255])),
                (
                    profile.model,
                    profile.dpi,
                    label.width_px,
                    label.height_px,
                    safe.top,
                ),
            ))
        })
        .await
        .unwrap();
        assert_eq!(rendered, (model, dpi, width, height, safe_top));

        let events = events.lock().unwrap();
        let identity = event_position(
            &events,
            |event| matches!(event, Event::Send { cmd, data } if *cmd == Cmd::PrinterInfo as u8 && data == &[8]),
        );
        let render = event_position(&events, |event| matches!(event, Event::Render));
        let density = event_position(
            &events,
            |event| matches!(event, Event::Send { cmd, .. } if *cmd == Cmd::SetDensity as u8),
        );
        let heartbeat = event_position(
            &events,
            |event| matches!(event, Event::Send { cmd, .. } if *cmd == Cmd::Heartbeat as u8),
        );
        let print_start = event_position(
            &events,
            |event| matches!(event, Event::Send { cmd, .. } if *cmd == Cmd::PrintStart as u8),
        );
        let close = event_position(&events, |event| matches!(event, Event::Close));
        assert!(
            identity < render,
            "identity must precede rendering: {events:?}"
        );
        assert!(
            render < density,
            "render must precede the first print mutation: {events:?}"
        );
        assert!(
            heartbeat < print_start,
            "preflight must precede print: {events:?}"
        );
        assert!(
            print_start < close,
            "close must follow the print: {events:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Close))
                .count(),
            1
        );
        assert_eq!(events.last(), Some(&Event::Close));

        let page_size = first_packet(&events, Cmd::SetPageSize);
        assert_eq!(
            u16::from_be_bytes([page_size[0], page_size[1]]),
            height as u16
        );
        assert_eq!(
            u16::from_be_bytes([page_size[2], page_size[3]]),
            width as u16
        );
    }
}

#[tokio::test]
async fn detected_experimental_profile_is_gated_before_render_and_still_closes() {
    for (model_id, auto_task) in [(4097, true), (785, false), (2304, true)] {
        let (session, events) = detected_session(model_id, auto_task, false).await;
        let error = run_rendered_session(
            session,
            crate::Density::NORMAL,
            |_profile| -> Result<(GrayImage, ())> {
                events.lock().unwrap().push(Event::Render);
                Ok((GrayImage::from_pixel(1, 1, Luma([255])), ()))
            },
        )
        .await
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("experimental"), "{message}");
        assert!(message.contains("--allow-experimental"), "{message}");

        let events = events.lock().unwrap();
        assert!(!events.iter().any(|event| matches!(event, Event::Render)));
        assert!(!events.iter().any(is_print_mutation));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Close))
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn detected_b1_over_usb_is_gated_before_render_and_still_closes() {
    let (session, events) = detected_session_over(4096, ConnPref::Usb, true, false).await;
    let render_events = Arc::clone(&events);
    let error = run_rendered_session(
        session,
        crate::Density::NORMAL,
        |_profile| -> Result<(GrayImage, ())> {
            render_events.lock().unwrap().push(Event::Render);
            Ok((GrayImage::from_pixel(1, 1, Luma([255])), ()))
        },
    )
    .await
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("over 'usb'"), "{message}");
    assert!(message.contains("--allow-experimental"), "{message}");
    let events = events.lock().unwrap();
    assert!(!events.iter().any(|event| matches!(event, Event::Render)));
    assert!(!events.iter().any(is_print_mutation));
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test(start_paused = true)]
async fn successful_print_reports_disconnect_failure() {
    let (session, events) = detected_session(4096, true, false).await;
    session.client.transport().fail_close();
    let error = run_rendered_session(session, crate::Density::NORMAL, |_profile| {
        Ok((GrayImage::from_pixel(8, 1, Luma([0])), ()))
    })
    .await
    .unwrap_err();
    assert!(error.to_string().contains("synthetic disconnect failure"));

    let events = events.lock().unwrap();
    let end_print = event_position(
        &events,
        |event| matches!(event, Event::Send { cmd, .. } if *cmd == Cmd::PrintEnd as u8),
    );
    let close = event_position(&events, |event| matches!(event, Event::Close));
    assert!(end_print < close);
    assert_eq!(
        events.iter().filter(|e| matches!(e, Event::Close)).count(),
        1
    );
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test(start_paused = true)]
async fn print_failure_survives_disconnect_failure() {
    let mut mock = crate::MockTransport::new();
    mock.fail_cmd(Cmd::PrintStart as u8, crate::PrinterFault::NO_PAPER.code());
    let (transport, events) = ObservedTransport::new(mock);
    transport.fail_close();
    let client = PrinterClient::new(transport, Model::B1).with_pacing(Pacing::INSTANT);
    let session = Session::<ObservedTransport>::finish_connect(
        client,
        ConnPref::Ble,
        PrintTarget {
            model: Model::B1,
            task: TaskSelection::Auto {
                default: PrintTask::B1,
            },
            allow_experimental: false,
        },
        IdentityDetail::Profile,
    )
    .await
    .unwrap();
    let error = run_rendered_session(session, crate::Density::NORMAL, |_profile| {
        Ok((GrayImage::from_pixel(8, 1, Luma([0])), ()))
    })
    .await
    .unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<crate::Error>(),
            Some(crate::Error::Printer(crate::PrinterFault::NO_PAPER))
        ),
        "{error:?}"
    );

    let events = events.lock().unwrap();
    assert!(events.iter().any(|event| {
        matches!(event, Event::Send { cmd, .. } if *cmd == Cmd::PrintStart as u8)
    }));
    assert!(!events.iter().any(|event| {
        matches!(event, Event::Send { cmd, .. } if *cmd == Cmd::PrintBitmapRow as u8)
    }));
    assert_eq!(
        events.iter().filter(|e| matches!(e, Event::Close)).count(),
        1
    );
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test]
async fn png_save_failure_closes_before_starting_a_print() {
    let dir = tempfile::tempdir().unwrap();
    let save_path = dir.path().join("occupied.png");
    // An existing directory fails deterministically, even with writable permissions.
    std::fs::create_dir(&save_path).unwrap();
    let (session, events) = detected_session(4096, true, false).await;
    let render_events = Arc::clone(&events);
    let error = run_rendered_session(session, crate::Density::NORMAL, |_profile| {
        render_events.lock().unwrap().push(Event::Render);
        let gray = GrayImage::from_pixel(8, 1, Luma([0]));
        gray.save(&save_path)
            .with_context(|| format!("save {}", save_path.display()))?;
        Ok((gray, ()))
    })
    .await
    .unwrap_err();
    assert!(error.to_string().contains("occupied.png"));
    assert!(save_path.is_dir());

    let events = events.lock().unwrap();
    assert!(events.iter().any(|e| matches!(e, Event::Render)));
    assert!(!events.iter().any(is_print_mutation));
    assert_eq!(
        events.iter().filter(|e| matches!(e, Event::Close)).count(),
        1
    );
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test]
async fn render_failure_still_closes_without_starting_a_print() {
    let (session, events) = detected_session(4097, true, true).await;
    let render_events = Arc::clone(&events);
    let error = run_rendered_session(
        session,
        crate::Density::NORMAL,
        |_profile| -> Result<(GrayImage, ())> {
            render_events.lock().unwrap().push(Event::Render);
            bail!("synthetic render failure")
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("synthetic render failure"));

    let events = events.lock().unwrap();
    assert!(events.iter().any(|event| matches!(event, Event::Render)));
    assert!(!events.iter().any(is_print_mutation));
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test]
async fn unknown_identity_cannot_fall_back_to_provisional_geometry() {
    let (session, events) = detected_session(0xffff, true, true).await;
    let render_events = Arc::clone(&events);
    let error = run_rendered_session(
        session,
        crate::Density::NORMAL,
        |_profile| -> Result<(GrayImage, ())> {
            render_events.lock().unwrap().push(Event::Render);
            Ok((GrayImage::from_pixel(1, 1, Luma([255])), ()))
        },
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unrecognized printer model id 65535")
    );

    let events = events.lock().unwrap();
    assert!(!events.iter().any(|event| matches!(event, Event::Render)));
    assert!(!events.iter().any(is_print_mutation));
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test]
async fn identity_query_failure_cannot_use_provisional_geometry() {
    let mut mock = crate::MockTransport::new();
    mock.fail_receives("identity transport failure");
    let (transport, events) = ObservedTransport::new(mock);
    let client = PrinterClient::new(transport, Model::B1).with_pacing(Pacing::INSTANT);
    let session = Session::<ObservedTransport>::finish_connect(
        client,
        ConnPref::Ble,
        PrintTarget {
            model: Model::B1,
            task: TaskSelection::Auto {
                default: PrintTask::B1,
            },
            allow_experimental: true,
        },
        IdentityDetail::Profile,
    )
    .await
    .unwrap();

    let render_events = Arc::clone(&events);
    let error = run_rendered_session(
        session,
        crate::Density::NORMAL,
        |_profile| -> Result<(GrayImage, ())> {
            render_events.lock().unwrap().push(Event::Render);
            Ok((GrayImage::from_pixel(1, 1, Luma([255])), ()))
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("printer identification failed"));

    let events = events.lock().unwrap();
    assert!(!events.iter().any(|event| matches!(event, Event::Render)));
    assert!(!events.iter().any(is_print_mutation));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Close))
            .count(),
        1
    );
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test]
async fn explicit_task_keeps_its_wire_shape_with_detected_geometry() {
    let (session, events) = detected_session(785, false, true).await;
    assert_eq!(session.profile().model, Model::B21Pro);
    assert_eq!(session.client.print_task(), PrintTask::B1);

    let rendered = run_rendered_session(session, crate::Density::NORMAL, |profile| {
        let label =
            LabelMm::parse("50x30")?.to_pixels(profile.max_width_px, profile.pixels_per_mm());
        Ok((
            GrayImage::from_pixel(label.width_px, label.height_px, Luma([255])),
            label,
        ))
    })
    .await
    .unwrap();
    assert_eq!((rendered.width_px, rendered.height_px), (591, 354));

    let events = events.lock().unwrap();
    assert_eq!(first_packet(&events, Cmd::PrintStart).len(), 7);
    let page_size = first_packet(&events, Cmd::SetPageSize);
    assert_eq!(page_size.len(), 6);
    assert_eq!(u16::from_be_bytes([page_size[0], page_size[1]]), 354);
    assert_eq!(u16::from_be_bytes([page_size[2], page_size[3]]), 591);
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test]
async fn detected_narrow_profile_qr_error_still_closes_before_printing() {
    let (session, events) = detected_session(2304, true, true).await;
    let error = run_rendered_session(
        session,
        crate::Density::NORMAL,
        |profile| -> Result<(GrayImage, ())> {
            let label =
                LabelMm::parse("50x30")?.to_pixels(profile.max_width_px, profile.pixels_per_mm());
            let gray = crate::make_qr_label_opts(&crate::label::QrLabelOptions {
                url: "https://example.com/42".into(),
                side_text: "ORDER 42".into(),
                label,
                safe: Config::default().resolve_safe_area(profile.pixels_per_mm()),
                text_side: crate::label::TextSide::Right,
                border: false,
                font_path: None,
                font_name: None,
                font_size: None,
            })?;
            Ok((gray, ()))
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("96x240px is too small"));

    let events = events.lock().unwrap();
    assert!(!events.iter().any(is_print_mutation));
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test]
async fn file_print_uses_detected_canvas_and_closes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.png");
    GrayImage::from_pixel(8, 8, Luma([0])).save(&path).unwrap();
    let (session, events) = detected_session(4097, true, true).await;

    run_file_session(
        session,
        &Config::default(),
        &path,
        PrintOptions {
            label: Some(LabelMm::parse("50x30").unwrap()),
            fill: false,
            trim: false,
            ..PrintOptions::default()
        },
        false,
    )
    .await
    .unwrap();

    let events = events.lock().unwrap();
    let page_size = first_packet(&events, Cmd::SetPageSize);
    assert_eq!(u16::from_be_bytes([page_size[0], page_size[1]]), 354);
    assert_eq!(u16::from_be_bytes([page_size[2], page_size[3]]), 567);
    assert_eq!(events.last(), Some(&Event::Close));
}

#[tokio::test]
async fn file_composition_failure_still_closes_before_printing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.png");
    GrayImage::from_pixel(8, 8, Luma([0])).save(&path).unwrap();
    let (session, events) = detected_session(4096, true, false).await;
    let cfg = Config {
        safe_area: Some(SafeArea {
            top: 100,
            bottom: 100,
            left: 100,
            right: 100,
        }),
        ..Config::default()
    };

    let error = run_file_session(
        session,
        &cfg,
        &path,
        PrintOptions {
            label: Some(LabelMm::parse("10x10").unwrap()),
            fill: false,
            trim: false,
            ..PrintOptions::default()
        },
        false,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("leaves no content"));

    let events = events.lock().unwrap();
    assert!(!events.iter().any(is_print_mutation));
    assert_eq!(events.last(), Some(&Event::Close));
}

#[test]
fn connected_default_safe_area_uses_detected_dpi() {
    let detected = crate::profile_for_model(Model::B1Pro);
    assert_eq!(
        connected_safe_area(&Config::default(), detected, false).top,
        12
    );

    let configured = Config {
        safe_area: Some(SafeArea {
            top: 3,
            bottom: 4,
            left: 5,
            right: 6,
        }),
        ..Config::default()
    };
    assert_eq!(
        connected_safe_area(&configured, detected, false),
        configured.safe_area.unwrap()
    );
    assert_eq!(
        connected_safe_area(&configured, detected, true),
        SafeArea::NONE
    );
}
