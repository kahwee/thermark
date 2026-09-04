//! Opening a printer session and running one job through it.

#[cfg(any(feature = "ble", feature = "serial"))]
use anyhow::Context;
use anyhow::{Result, bail};
use std::path::Path;
use thermark::config::{Config, ConnPref};
use thermark::print_task::PrintTask;
#[cfg(any(feature = "ble", feature = "serial"))]
use thermark::printer::Pacing;
use thermark::printer::{PrintOptions, PrinterClient, PrinterSummary};
use thermark::profile::PrinterProfile;
use thermark::protocol::Model;
#[cfg(feature = "ble")]
use thermark::transport::BleTransport;
#[cfg(feature = "serial")]
use thermark::transport::SerialTransport;
use thermark::transport::Transport;

use super::args::{ConnArgs, ResolvedConn, TaskArgs};

#[derive(Debug, Clone, Copy)]
pub struct PrintTarget {
    pub model: Model,
    pub task: PrintTask,
    pub task_explicit: bool,
    pub allow_experimental: bool,
}

pub fn resolve_target(cfg: &Config, model: Option<Model>, args: &TaskArgs) -> Result<PrintTarget> {
    let model = cfg.resolve_model(model);
    let task = resolve_task(model, args)?;
    Ok(PrintTarget {
        model,
        task,
        task_explicit: args.task.is_some(),
        allow_experimental: args.allow_experimental,
    })
}

/// Row pacing, overridable for diagnosing dense-page truncation.
///
/// `THERMARK_SLOW=1` selects [`Pacing::CAREFUL`]. Dense pages come back
/// truncated while sparse ones do not, which points at the printer dropping
/// data rather than at a printable-area limit; this makes that testable
/// without a rebuild.
#[cfg(any(feature = "ble", feature = "serial"))]
fn pacing_from_env() -> Pacing {
    match std::env::var("THERMARK_SLOW") {
        Ok(v) if !v.trim().is_empty() && v != "0" => {
            eprintln!("pacing: CAREFUL (THERMARK_SLOW set)");
            Pacing::CAREFUL
        }
        _ => Pacing::REAL,
    }
}

/// CLI transport sum type; the protocol client remains transport-agnostic.
pub enum AnyTransport {
    #[cfg(feature = "ble")]
    Ble(BleTransport),
    #[cfg(feature = "serial")]
    Usb(SerialTransport),
}

impl Transport for AnyTransport {
    #[allow(unused_variables)]
    async fn send_raw(&mut self, data: &[u8]) -> thermark::Result<()> {
        match self {
            #[cfg(feature = "ble")]
            Self::Ble(transport) => transport.send_raw(data).await,
            #[cfg(feature = "serial")]
            Self::Usb(transport) => transport.send_raw(data).await,
            #[cfg(not(any(feature = "ble", feature = "serial")))]
            _ => unreachable!("AnyTransport has no variants"),
        }
    }

    #[allow(unused_variables)]
    async fn recv_raw(&mut self, wait: std::time::Duration) -> thermark::Result<Vec<u8>> {
        match self {
            #[cfg(feature = "ble")]
            Self::Ble(transport) => transport.recv_raw(wait).await,
            #[cfg(feature = "serial")]
            Self::Usb(transport) => transport.recv_raw(wait).await,
            #[cfg(not(any(feature = "ble", feature = "serial")))]
            _ => unreachable!("AnyTransport has no variants"),
        }
    }

    async fn close(&mut self) -> thermark::Result<()> {
        match self {
            #[cfg(feature = "ble")]
            Self::Ble(transport) => transport.close().await,
            #[cfg(feature = "serial")]
            Self::Usb(transport) => transport.close().await,
            #[cfg(not(any(feature = "ble", feature = "serial")))]
            _ => unreachable!("AnyTransport has no variants"),
        }
    }
}

/// An open BLE or USB printer session.
pub struct Session<T: Transport = AnyTransport> {
    client: PrinterClient<T>,
    allow_experimental: bool,
}

#[derive(Debug, Clone, Copy)]
enum IdentityDetail {
    Profile,
    Full,
}

fn combine_job_and_close<T>(job: Result<T>, close: Result<()>) -> Result<T> {
    match (job, close) {
        (Err(job), Err(close)) => {
            tracing::warn!(error = %close, "printer shutdown failed after operation error");
            Err(job)
        }
        (Err(job), Ok(())) => Err(job),
        (Ok(_), Err(close)) => Err(close),
        (Ok(value), Ok(())) => Ok(value),
    }
}

impl Session<AnyTransport> {
    pub async fn connect(
        conn: &ResolvedConn,
        model: Model,
        task: PrintTask,
        auto_task: bool,
        allow_experimental: bool,
    ) -> Result<Self> {
        Self::connect_with_identity(
            conn,
            model,
            task,
            auto_task,
            allow_experimental,
            IdentityDetail::Profile,
        )
        .await
    }

    /// Open a session and retain the full identity report for presentation.
    ///
    /// Normal printing only needs [`Self::connect`]; this variant keeps the
    /// firmware and hardware metadata queries used by the `identify` command.
    pub async fn connect_detailed(
        conn: &ResolvedConn,
        model: Model,
        task: PrintTask,
        auto_task: bool,
        allow_experimental: bool,
    ) -> Result<Self> {
        Self::connect_with_identity(
            conn,
            model,
            task,
            auto_task,
            allow_experimental,
            IdentityDetail::Full,
        )
        .await
    }

    #[allow(unused_variables)]
    async fn connect_with_identity(
        conn: &ResolvedConn,
        model: Model,
        task: PrintTask,
        auto_task: bool,
        allow_experimental: bool,
        identity_detail: IdentityDetail,
    ) -> Result<Self> {
        match conn.conn {
            ConnPref::Ble => {
                #[cfg(not(feature = "ble"))]
                bail!("this thermark binary was built without Bluetooth support");
                #[cfg(feature = "ble")]
                {
                    let ble = BleTransport::connect_with(
                        &conn.addr,
                        std::time::Duration::from_secs(conn.scan_secs),
                        conn.match_mode,
                    )
                    .await
                    .context("BLE connect")?;
                    let client = PrinterClient::new_with_task(AnyTransport::Ble(ble), model, task)
                        .with_pacing(pacing_from_env());
                    Self::finish_connect(client, auto_task, allow_experimental, identity_detail)
                        .await
                }
            }
            ConnPref::Usb => {
                #[cfg(not(feature = "serial"))]
                bail!("this thermark binary was built without USB serial support");
                #[cfg(feature = "serial")]
                {
                    let ser = SerialTransport::open(&conn.addr)
                        .with_context(|| format!("open serial {}", conn.addr))?;
                    let client = PrinterClient::new_with_task(AnyTransport::Usb(ser), model, task)
                        .with_pacing(pacing_from_env());
                    Self::finish_connect(client, auto_task, allow_experimental, identity_detail)
                        .await
                }
            }
        }
    }
}

impl<T: Transport> Session<T> {
    #[cfg_attr(not(any(feature = "ble", feature = "serial")), allow(dead_code))]
    async fn finish_connect(
        mut client: PrinterClient<T>,
        auto_task: bool,
        allow_experimental: bool,
        identity_detail: IdentityDetail,
    ) -> Result<Self> {
        let identity_result = match identity_detail {
            IdentityDetail::Profile => client.identify_profile().await,
            IdentityDetail::Full => client.identify().await,
        };
        let identity = match identity_result {
            Ok(identity) => Some(identity),
            Err(error) => {
                tracing::warn!(error = %error, "printer identification failed");
                None
            }
        };
        if let Some(identity) = &identity {
            if let Some(profile) = client.apply_identity(identity, auto_task) {
                tracing::info!(model = %profile.model, model_id = identity.model_id, dpi = profile.dpi, task = ?profile.task, "identified printer");
            } else {
                tracing::warn!(
                    model_id = identity.model_id,
                    "printer model is not in the profile registry"
                );
            }
        }
        Ok(Self {
            client,
            allow_experimental,
        })
    }

    pub fn identity(&self) -> Option<&thermark::PrinterIdentity> {
        self.client.identity()
    }

    pub fn profile(&self) -> &'static PrinterProfile {
        self.client.profile()
    }

    pub async fn fetch_summary(&mut self) -> Result<PrinterSummary> {
        Ok(self.client.fetch_summary().await?)
    }

    pub async fn print_image_file_opts(&mut self, path: &Path, opts: PrintOptions) -> Result<()> {
        self.ensure_print_allowed()?;
        self.client.print_image_file_opts(path, opts).await?;
        Ok(())
    }

    /// Render against the connected printer's effective physical profile,
    /// then print the exact grayscale image that renderer produced.
    ///
    /// Identification and the experimental-path gate both happen before the
    /// renderer is called, so DPI-dependent geometry cannot be derived from a
    /// stale configured model. The caller owns closing the session so the same
    /// shutdown path covers render, save, and print failures.
    async fn render_and_print_gray<R>(
        &mut self,
        density: thermark::Density,
        render: impl FnOnce(&'static PrinterProfile) -> Result<(image::GrayImage, R)>,
    ) -> Result<R> {
        self.ensure_print_allowed()?;
        let (gray, rendered) = render(self.client.profile())?;
        self.client.print_gray_image(&gray, density).await?;
        Ok(rendered)
    }

    fn ensure_print_allowed(&self) -> Result<()> {
        let identity = self.client.identity().ok_or_else(|| {
            anyhow::anyhow!(
                "printer identification failed; refusing to print with provisional geometry"
            )
        })?;
        if thermark::profile_for_identity(identity).is_none() {
            bail!(
                "unrecognized printer model id {}; add a checked device profile before printing",
                identity.model_id
            );
        }
        let model = self.client.model();
        let task = self.client.print_task();
        ensure_print_path_allowed(model, task, self.allow_experimental)?;
        if !thermark::profile_for_model(model).print_path_hardware_tested(task) {
            eprintln!(
                "warning: printer profile '{model}' with print task '{task}' is experimental \
                 (not hardware-tested in this project)"
            );
        }
        Ok(())
    }

    /// Release the link. [`BleTransport`]'s `Drop` is only a backstop.
    pub async fn finish(self) -> Result<()> {
        self.client.close().await.map_err(anyhow::Error::from)
    }
}

async fn run_rendered_session<T: Transport, R>(
    mut session: Session<T>,
    density: thermark::Density,
    render: impl FnOnce(&'static PrinterProfile) -> Result<(image::GrayImage, R)>,
) -> Result<R> {
    let result = session.render_and_print_gray(density, render).await;
    let close_result = session.finish().await;
    combine_job_and_close(result, close_result)
}

fn connected_safe_area(
    cfg: &Config,
    profile: &PrinterProfile,
    full_bleed: bool,
) -> thermark::geometry::SafeArea {
    if full_bleed {
        thermark::geometry::SafeArea::NONE
    } else {
        cfg.resolve_safe_area(profile.pixels_per_mm())
    }
}

async fn run_file_session<T: Transport>(
    mut session: Session<T>,
    cfg: &Config,
    path: &Path,
    mut opts: PrintOptions,
    full_bleed: bool,
) -> Result<()> {
    // A saved safe area is already expressed in pixels and remains exact.
    // The default registration inset is physical (1 mm), so resolve it again
    // at the detected DPI rather than retaining the provisional model's value.
    opts.safe = connected_safe_area(cfg, session.profile(), full_bleed);
    let result = session.print_image_file_opts(path, opts).await;
    let close_result = session.finish().await;
    combine_job_and_close(result, close_result)
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
    ensure_target_print_allowed(target)?;
    let conn = conn.resolve(cfg)?;
    let session = Session::connect(
        &conn,
        target.model,
        target.task,
        !target.task_explicit,
        target.allow_experimental,
    )
    .await?;
    run_file_session(session, cfg, path, opts, full_bleed).await
}

pub async fn render_and_print_gray_resolved<R>(
    cfg: &Config,
    conn: &ConnArgs,
    target: PrintTarget,
    density: thermark::Density,
    render: impl FnOnce(&'static PrinterProfile) -> Result<(image::GrayImage, R)>,
) -> Result<R> {
    ensure_target_print_allowed(target)?;
    let resolved_conn = conn.resolve(cfg)?;
    let session = Session::connect(
        &resolved_conn,
        target.model,
        target.task,
        !target.task_explicit,
        target.allow_experimental,
    )
    .await?;
    run_rendered_session(session, density, render).await
}

/// Resolve the print task: `--task` wins, otherwise use the profile default.
///
/// This only selects protocol metadata. The experimental-task gate belongs at
/// the actual printing boundary so offline previews and renders can use every
/// model profile without opting in to hardware writes.
pub fn resolve_task(model: Model, args: &TaskArgs) -> Result<PrintTask> {
    args.task.or_else(|| PrintTask::for_model(model)).ok_or_else(|| {
        anyhow::anyhow!(
            "model '{model}' has no verified default print task; identify it and pass --task explicitly"
        )
    })
}

/// Reject an unverified configured task before opening a hardware connection.
/// The session checks again after identification because auto-detection may
/// replace the configured task with a different protocol sequence.
fn ensure_target_print_allowed(target: PrintTarget) -> Result<()> {
    ensure_print_path_allowed(target.model, target.task, target.allow_experimental)
}

/// Require opt-in unless this exact physical profile and wire task form the
/// project's hardware-verified path.
fn ensure_print_path_allowed(
    model: Model,
    task: PrintTask,
    allow_experimental: bool,
) -> Result<()> {
    let profile = thermark::profile_for_model(model);
    if !profile.print_path_hardware_tested(task) && !allow_experimental {
        bail!(
            "printer profile '{model}' with print task '{task}' is experimental \
             (not hardware-tested in this project). \
             Re-run with --allow-experimental if you accept the risk, \
             or use the hardware-tested B1 profile and task. See: thermark tasks"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use thermark::geometry::{LabelMm, SafeArea};
    use thermark::printer::Pacing;
    use thermark::protocol::Cmd;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        Send { cmd: u8, data: Vec<u8> },
        Render,
        Close,
    }

    type Events = Arc<Mutex<Vec<Event>>>;

    struct ObservedTransport {
        inner: thermark::MockTransport,
        events: Events,
    }

    impl ObservedTransport {
        fn with_model_id(model_id: u16) -> (Self, Events) {
            let mut inner = thermark::MockTransport::new();
            inner.set_model_id(model_id);
            Self::new(inner)
        }

        fn new(inner: thermark::MockTransport) -> (Self, Events) {
            let events = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    inner,
                    events: Arc::clone(&events),
                },
                events,
            )
        }
    }

    impl Transport for ObservedTransport {
        async fn send_raw(&mut self, data: &[u8]) -> thermark::Result<()> {
            let packet = thermark::Packet::decode(data).expect("client must send a valid packet");
            self.events.lock().unwrap().push(Event::Send {
                cmd: packet.cmd,
                data: packet.data,
            });
            self.inner.send_raw(data).await
        }

        async fn recv_raw(&mut self, wait: Duration) -> thermark::Result<Vec<u8>> {
            self.inner.recv_raw(wait).await
        }

        async fn close(&mut self) -> thermark::Result<()> {
            self.events.lock().unwrap().push(Event::Close);
            self.inner.close().await
        }
    }

    async fn detected_session(
        model_id: u16,
        auto_task: bool,
        allow_experimental: bool,
    ) -> (Session<ObservedTransport>, Events) {
        let (transport, events) = ObservedTransport::with_model_id(model_id);
        let client = PrinterClient::new_with_task(transport, Model::B1, PrintTask::B1)
            .with_pacing(Pacing::INSTANT);
        let session = Session::<ObservedTransport>::finish_connect(
            client,
            auto_task,
            allow_experimental,
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

    fn args(task: Option<PrintTask>, allow: bool) -> TaskArgs {
        TaskArgs {
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
        let target =
            resolve_target(&Config::default(), Some(Model::D110), &args(None, false)).unwrap();
        assert!(ensure_target_print_allowed(target).is_err());

        let allowed = PrintTarget {
            allow_experimental: true,
            ..target
        };
        assert!(ensure_target_print_allowed(allowed).is_ok());
    }

    #[test]
    fn detected_experimental_profile_with_explicit_b1_task_stays_gated() {
        let mut client =
            PrinterClient::new_with_task(thermark::MockTransport::new(), Model::B1, PrintTask::B1);
        let identity = thermark::PrinterIdentity {
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
        assert!(ensure_print_path_allowed(client.model(), client.print_task(), false).is_err());
        assert!(ensure_print_path_allowed(client.model(), client.print_task(), true).is_ok());
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
            let rendered = run_rendered_session(session, thermark::Density::NORMAL, |profile| {
                let label = LabelMm::parse("50x30")?
                    .to_pixels(profile.max_width_px, profile.pixels_per_mm());
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
                thermark::Density::NORMAL,
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
    async fn render_failure_still_closes_without_starting_a_print() {
        let (session, events) = detected_session(4097, true, true).await;
        let render_events = Arc::clone(&events);
        let error = run_rendered_session(
            session,
            thermark::Density::NORMAL,
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
            thermark::Density::NORMAL,
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
        let mut mock = thermark::MockTransport::new();
        mock.fail_receives("identity transport failure");
        let (transport, events) = ObservedTransport::new(mock);
        let client = PrinterClient::new(transport, Model::B1).with_pacing(Pacing::INSTANT);
        let session = Session::<ObservedTransport>::finish_connect(
            client,
            true,
            true,
            IdentityDetail::Profile,
        )
        .await
        .unwrap();

        let render_events = Arc::clone(&events);
        let error = run_rendered_session(
            session,
            thermark::Density::NORMAL,
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

        let rendered = run_rendered_session(session, thermark::Density::NORMAL, |profile| {
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
            thermark::Density::NORMAL,
            |profile| -> Result<(GrayImage, ())> {
                let label = LabelMm::parse("50x30")?
                    .to_pixels(profile.max_width_px, profile.pixels_per_mm());
                let gray = thermark::make_qr_label_opts(&thermark::label::QrLabelOptions {
                    url: "https://example.com/42".into(),
                    side_text: "ORDER 42".into(),
                    label,
                    safe: Config::default().resolve_safe_area(profile.pixels_per_mm()),
                    text_side: thermark::label::TextSide::Right,
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
        let detected = thermark::profile_for_model(Model::B1Pro);
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
}
