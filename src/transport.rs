//! Transports: Bluetooth LE (macOS-friendly) and USB serial.
//!
//! Enable with Cargo features:
//! - `ble` — [`BleTransport`] (btleplug)
//! - `serial` — [`SerialTransport`]

use crate::errors::{Error, Result};
use crate::packet::Packet;
use std::fmt;
use std::time::Duration;
use tracing::debug;

/// Common packet transport used by [`crate::printer::PrinterClient`].
///
/// Native `async fn` in traits (no `async-trait`); not object-safe by design.
pub trait Transport: Send {
    fn send_raw(&mut self, data: &[u8]) -> impl std::future::Future<Output = Result<()>> + Send;

    fn recv_packets(
        &mut self,
        wait: Duration,
    ) -> impl std::future::Future<Output = Result<Vec<Packet>>> + Send;

    fn send_packet(
        &mut self,
        packet: &Packet,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        async {
            let bytes = packet.encode()?;
            debug!(bytes = %hex::encode(&bytes), "TX");
            self.send_raw(&bytes).await
        }
    }
}

// ─── BLE device selection (pure; no btleplug) ───────────────────────────────

/// How `-a` / `THERMARK_ADDR` is matched to a BLE peripheral.
///
/// Default is [`BleMatchMode::Exact`] so short selectors cannot latch onto the
/// wrong nearby device. Use [`BleMatchMode::Fuzzy`] only when you intentionally
/// want substring matching (`--fuzzy` on the CLI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BleMatchMode {
    /// Case-insensitive exact advertising name **or** exact peripheral id.
    #[default]
    Exact,
    /// Substring name / partial id (legacy). Can match the wrong device.
    Fuzzy,
}

impl BleMatchMode {
    pub fn from_fuzzy(fuzzy: bool) -> Self {
        if fuzzy { Self::Fuzzy } else { Self::Exact }
    }
}

/// A BLE peripheral seen during a scan.
///
/// `name` is `None` when the device advertises no local name — previously a
/// `"(no name)"` sentinel string that had to be excluded from matching at every
/// comparison site, and which the connect path never actually produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleCandidate {
    pub id: String,
    pub name: Option<String>,
}

impl BleCandidate {
    pub fn new(id: impl Into<String>, name: Option<String>) -> Self {
        let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        Self {
            id: id.into(),
            name,
        }
    }

    /// Advertising name, or a placeholder for display only — never for matching.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("(no name)")
    }

    /// True if the advertised name looks like a pocket label printer.
    ///
    /// Single source of truth for this heuristic: scan filtering, fuzzy-match
    /// scoring, and `scan --save` ranking all consult it.
    pub fn looks_like_label_printer(&self) -> bool {
        self.name
            .as_deref()
            .map(name_looks_like_label_printer)
            .unwrap_or(false)
    }
}

impl fmt::Display for BleCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            Some(n) => write!(f, "{n} ({})", self.id),
            None => write!(f, "{}", self.id),
        }
    }
}

/// Name prefixes/substrings used by common pocket thermal label printers.
const PRINTER_NAME_MARKERS: &[&str] = &["b1", "b21", "b18", "d11", "d110", "niim", "jc-"];

/// Whether an advertising name looks like a label printer (case-insensitive).
pub fn name_looks_like_label_printer(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    PRINTER_NAME_MARKERS.iter().any(|m| name.contains(m))
}

/// Score how well `selector` matches a scanned peripheral. Higher is better.
///
/// Returns `None` if this mode does not treat the device as a match.
pub fn score_ble_candidate(
    selector: &str,
    candidate: &BleCandidate,
    mode: BleMatchMode,
) -> Option<i32> {
    let sel = selector.trim().to_ascii_lowercase();
    if sel.is_empty() {
        return None;
    }
    let id_l = candidate.id.to_ascii_lowercase();
    let name_l = candidate.name.as_ref().map(|n| n.to_ascii_lowercase());

    let id_exact = id_l == sel;
    let name_exact = name_l.as_deref() == Some(sel.as_str());

    match mode {
        BleMatchMode::Exact => {
            if id_exact {
                Some(1000)
            } else if name_exact {
                Some(900)
            } else {
                None
            }
        }
        BleMatchMode::Fuzzy => {
            let mut score = 0i32;
            if id_exact {
                score += 1000;
            }
            if name_exact {
                score += 900;
            }
            if name_l.as_deref().is_some_and(|n| n.contains(&sel)) {
                score += 100;
            }
            // Partial UUID/id only when selector is long enough to be intentional.
            if id_l.contains(&sel) && sel.len() >= 8 {
                score += 50;
            }
            if candidate.looks_like_label_printer() {
                score += 30;
            }
            // Require a real selector hit (not printer-name bonus alone).
            if score < 100 { None } else { Some(score) }
        }
    }
}

/// Choose the best matching candidate.
///
/// Fails when nothing matches, or when two candidates share the top score
/// (ambiguous fuzzy match).
pub fn select_ble_candidate(
    selector: &str,
    candidates: &[BleCandidate],
    mode: BleMatchMode,
) -> Result<BleCandidate> {
    let mut ranked: Vec<(i32, &BleCandidate)> = candidates
        .iter()
        .filter_map(|c| score_ble_candidate(selector, c, mode).map(|s| (s, c)))
        .collect();

    if ranked.is_empty() {
        let hint = match mode {
            BleMatchMode::Exact => "Substring match: add --fuzzy (can pick the wrong device).",
            BleMatchMode::Fuzzy => "No substring matched either.",
        };
        return Err(Error::transport(format!(
            "no BLE device matching '{selector}' ({mode:?} match). Run `thermark scan` first \
             (on macOS the id is a UUID, not a classic MAC). \
             Tip: use the full advertising name, e.g. -a \"B1-YourPrinter\". {hint}"
        )));
    }

    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    let best = ranked[0].0;
    let ties: Vec<_> = ranked.iter().filter(|(s, _)| *s == best).collect();
    if ties.len() > 1 {
        let list = ties
            .iter()
            .map(|(_, c)| c.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::transport(format!(
            "ambiguous BLE match for '{selector}' (score {best}): {list}. \
             Use the full unique name or peripheral id."
        )));
    }

    Ok(ranked[0].1.clone())
}

// ─── BLE ────────────────────────────────────────────────────────────────────

#[cfg(feature = "ble")]
mod ble {
    use super::*;
    use btleplug::api::{
        Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
    };
    use btleplug::platform::{Adapter, Manager, Peripheral};
    use futures::stream::StreamExt;
    use std::collections::HashMap;
    use tokio::runtime::RuntimeFlavor;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, timeout};
    use tracing::{debug, info};
    use uuid::Uuid;

    /// BLE service / characteristic used by common pocket thermal label printers
    /// (community reverse-engineering; same UUIDs as B1-class devices).
    pub const PRINTER_SERVICE: Uuid = Uuid::from_u128(0xe7810a71_73ae_499d_8c15_faa9aef0c3f2);
    pub const PRINTER_CHAR: Uuid = Uuid::from_u128(0xbef8d6c9_9c21_4c9e_b632_bd58c1009f9f);
    /// Back-compat aliases
    pub const NIIMBOT_SERVICE: Uuid = PRINTER_SERVICE;
    pub const NIIMBOT_CHAR: Uuid = PRINTER_CHAR;

    pub struct BleTransport {
        peripheral: Peripheral,
        characteristic: Characteristic,
        /// Acknowledged when the characteristic allows it — see
        /// [`choose_write_type`].
        write_type: WriteType,
        rx: mpsc::UnboundedReceiver<Vec<u8>>,
        rx_buf: Vec<u8>,
        notify_task: Option<tokio::task::JoinHandle<()>>,
        /// True after explicit or Drop-time teardown (avoids double disconnect).
        closed: bool,
    }

    /// A scanned peripheral plus its signal strength.
    #[derive(Debug, Clone)]
    pub struct BleDeviceInfo {
        pub candidate: BleCandidate,
        pub rssi: Option<i16>,
    }

    impl BleDeviceInfo {
        pub fn id(&self) -> &str {
            &self.candidate.id
        }

        pub fn display_name(&self) -> &str {
            self.candidate.display_name()
        }
    }

    impl BleTransport {
        /// Scan for B1-class label-printer peripherals.
        ///
        /// Devices advertising the printer GATT service are included even when
        /// their name is unrecognized, so an unknown model is still findable.
        pub async fn scan(duration: Duration) -> Result<Vec<BleDeviceInfo>> {
            let adapter = default_adapter().await?;
            adapter
                .start_scan(ScanFilter::default())
                .await
                .map_err(|e| Error::transport(format!("BLE start_scan: {e}")))?;
            sleep(duration).await;
            let peris = adapter
                .peripherals()
                .await
                .map_err(|e| Error::transport(format!("BLE peripherals: {e}")))?;
            let mut found: HashMap<String, BleDeviceInfo> = HashMap::new();
            for p in peris {
                let Some(props) = p
                    .properties()
                    .await
                    .map_err(|e| Error::transport(format!("BLE properties: {e}")))?
                else {
                    continue;
                };
                let candidate = BleCandidate::new(p.id().to_string(), props.local_name);
                let interesting = candidate.looks_like_label_printer()
                    || props.services.contains(&PRINTER_SERVICE);
                if interesting {
                    found.insert(
                        candidate.id.clone(),
                        BleDeviceInfo {
                            candidate,
                            rssi: props.rssi,
                        },
                    );
                }
            }
            adapter.stop_scan().await.ok();
            let mut list: Vec<_> = found.into_values().collect();
            list.sort_by(|a, b| a.display_name().cmp(b.display_name()));
            Ok(list)
        }

        /// Connect with exact name or id match (safe default).
        pub async fn connect(selector: &str, scan_for: Duration) -> Result<Self> {
            Self::connect_with(selector, scan_for, BleMatchMode::Exact).await
        }

        /// Connect using [`BleMatchMode`] (`Exact` default, or `Fuzzy` substring).
        pub async fn connect_with(
            selector: &str,
            scan_for: Duration,
            mode: BleMatchMode,
        ) -> Result<Self> {
            let adapter = default_adapter().await?;
            adapter
                .start_scan(ScanFilter::default())
                .await
                .map_err(|e| Error::transport(format!("BLE start_scan: {e}")))?;
            sleep(scan_for).await;
            adapter.stop_scan().await.ok();

            let peris = adapter
                .peripherals()
                .await
                .map_err(|e| Error::transport(format!("BLE peripherals: {e}")))?;

            let mut catalog: Vec<(BleCandidate, Peripheral)> = Vec::new();
            for p in peris {
                let props = p
                    .properties()
                    .await
                    .map_err(|e| Error::transport(format!("BLE properties: {e}")))?;
                let name = props.as_ref().and_then(|pr| pr.local_name.clone());
                catalog.push((BleCandidate::new(p.id().to_string(), name), p));
            }

            let candidates: Vec<BleCandidate> = catalog.iter().map(|(c, _)| c.clone()).collect();
            let winner = select_ble_candidate(selector, &candidates, mode)?;

            let peripheral = catalog
                .into_iter()
                .find(|(c, _)| c.id == winner.id)
                .map(|(_, p)| p)
                .ok_or_else(|| Error::transport("internal: matched BLE device vanished"))?;

            info!(
                name = %winner.display_name(),
                id = %peripheral.id(),
                ?mode,
                "connecting"
            );

            peripheral.connect().await.map_err(|e| {
                Error::transport(format!(
                    "BLE connect failed: {e}. \
                     Only one app can use the printer — quit the official label app, \
                     then retry. If it still fails: power-cycle the printer, move closer, \
                     and use the full name from `thermark scan` (exact match by default)."
                ))
            })?;
            peripheral.discover_services().await.map_err(|e| {
                Error::transport(format!(
                    "discover services failed: {e}. Wrong device or incomplete BLE connection — \
                     run `thermark scan` and pass -a with the full advertising name."
                ))
            })?;

            let characteristic = find_printer_char(&peripheral).await?;

            let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
            peripheral
                .subscribe(&characteristic)
                .await
                .map_err(|e| Error::transport(format!("subscribe: {e}")))?;

            let mut notif = peripheral
                .notifications()
                .await
                .map_err(|e| Error::transport(format!("notifications stream: {e}")))?;
            let notify_task = tokio::spawn(async move {
                while let Some(n) = notif.next().await {
                    let _ = tx.send(n.value);
                }
            });

            sleep(Duration::from_millis(200)).await;

            Ok(Self {
                peripheral,
                write_type: choose_write_type(&characteristic),
                characteristic,
                rx,
                rx_buf: Vec::new(),
                notify_task: Some(notify_task),
                closed: false,
            })
        }

        /// Graceful teardown: abort notify task and disconnect the peripheral.
        ///
        /// Safe to call more than once. Also runs best-effort from [`Drop`] if
        /// the transport is abandoned after a mid-job error.
        pub async fn disconnect(mut self) -> Result<()> {
            self.close().await;
            Ok(())
        }

        async fn close(&mut self) {
            if self.closed {
                return;
            }
            self.closed = true;
            if let Some(task) = self.notify_task.take() {
                task.abort();
            }
            if let Err(e) = self.peripheral.disconnect().await {
                debug!(error = %e, "BLE disconnect (ignored)");
            } else {
                debug!("BLE disconnected");
            }
        }
    }

    impl Drop for BleTransport {
        /// Backstop teardown for paths that skip [`BleTransport::disconnect`]
        /// — a mid-job error, or the whole future being dropped on Ctrl-C.
        ///
        /// This *blocks* on a multi-threaded runtime rather than spawning a
        /// detached task. A detached task is not guaranteed to run at all: the
        /// common case is `main` returning right after an error, which shuts
        /// the runtime down before the task is ever polled, leaving the printer
        /// connected and holding the single-client BLE lock.
        fn drop(&mut self) {
            if self.closed {
                return;
            }
            self.closed = true;
            if let Some(task) = self.notify_task.take() {
                task.abort();
            }

            let Ok(handle) = tokio::runtime::Handle::try_current() else {
                // No runtime left to drive the disconnect; the OS tears the
                // link down when the process exits.
                debug!("BLE transport dropped outside a runtime");
                return;
            };
            let peripheral = self.peripheral.clone();

            match handle.runtime_flavor() {
                RuntimeFlavor::MultiThread => {
                    // `block_in_place` moves this off the async worker so the
                    // runtime stays live while we wait for the disconnect.
                    tokio::task::block_in_place(|| {
                        handle.block_on(async {
                            match peripheral.disconnect().await {
                                Ok(()) => debug!("BLE disconnected on drop"),
                                Err(e) => debug!(error = %e, "BLE disconnect on drop (ignored)"),
                            }
                        });
                    });
                }
                // Single-threaded runtimes (e.g. `#[tokio::test]`) cannot block
                // without deadlocking, so best-effort is all that is available.
                _ => {
                    handle.spawn(async move {
                        let _ = peripheral.disconnect().await;
                    });
                }
            }
        }
    }

    impl Transport for BleTransport {
        async fn send_raw(&mut self, data: &[u8]) -> Result<()> {
            const CHUNK: usize = 180;
            for chunk in data.chunks(CHUNK) {
                self.peripheral
                    .write(&self.characteristic, chunk, self.write_type)
                    .await
                    .map_err(|e| Error::transport(format!("BLE write: {e}")))?;
                sleep(Duration::from_millis(5)).await;
            }
            Ok(())
        }

        async fn recv_packets(&mut self, wait: Duration) -> Result<Vec<Packet>> {
            let deadline = tokio::time::Instant::now() + wait;
            loop {
                while let Ok(chunk) = self.rx.try_recv() {
                    debug!(bytes = %hex::encode(&chunk), "RX chunk");
                    self.rx_buf.extend_from_slice(&chunk);
                }
                let packets = Packet::drain_buffer(&mut self.rx_buf);
                if !packets.is_empty() {
                    for p in &packets {
                        debug!(
                            cmd = format_args!("{:#04x}", p.cmd),
                            data = %hex::encode(&p.data),
                            "RX pkt"
                        );
                    }
                    return Ok(packets);
                }
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Ok(vec![]);
                }
                match timeout(remaining, self.rx.recv()).await {
                    Ok(Some(chunk)) => {
                        debug!(bytes = %hex::encode(&chunk), "RX chunk");
                        self.rx_buf.extend_from_slice(&chunk);
                    }
                    Ok(None) => {
                        return Err(Error::transport("BLE notification channel closed"));
                    }
                    Err(_) => return Ok(vec![]),
                }
            }
        }
    }

    /// Write type for row data.
    ///
    /// Unacknowledged, matching the reference implementation (the protocol reference sends
    /// `writeValueWithoutResponse` with a fixed inter-packet delay). Acknowledged
    /// writes were tried against a real B1 and made no difference to the
    /// truncation being investigated, so the deviation was not worth keeping.
    fn choose_write_type(_characteristic: &Characteristic) -> WriteType {
        WriteType::WithoutResponse
    }

    async fn default_adapter() -> Result<Adapter> {
        let manager = Manager::new()
            .await
            .map_err(|e| Error::transport(format!("btleplug Manager: {e}")))?;
        let adapters = manager
            .adapters()
            .await
            .map_err(|e| Error::transport(format!("list adapters: {e}")))?;
        adapters
            .into_iter()
            .next()
            .ok_or_else(|| Error::transport("no Bluetooth adapter found — is Bluetooth enabled?"))
    }

    /// True if a host Bluetooth adapter is available (does not require a printer).
    pub async fn bluetooth_available() -> Result<String> {
        let adapter = default_adapter().await?;
        let name = adapter
            .adapter_info()
            .await
            .unwrap_or_else(|_| "Bluetooth adapter".into());
        Ok(name)
    }

    async fn find_printer_char(peripheral: &Peripheral) -> Result<Characteristic> {
        let chars = peripheral.characteristics();

        if let Some(c) = chars.iter().find(|c| c.uuid == PRINTER_CHAR) {
            return Ok(c.clone());
        }

        Err(Error::transport(format!(
            "printer characteristic {PRINTER_CHAR} not found (wrong device?). \
             Discovered characteristics:\n{}",
            chars
                .iter()
                .map(|c| format!("  {} props={:?}", c.uuid, c.properties))
                .collect::<Vec<_>>()
                .join("\n")
        )))
    }
}

#[cfg(feature = "ble")]
pub use ble::{
    BleDeviceInfo, BleTransport, NIIMBOT_CHAR, NIIMBOT_SERVICE, PRINTER_CHAR, PRINTER_SERVICE,
    bluetooth_available,
};

#[cfg(test)]
mod ble_match_tests {
    use super::*;

    fn named(id: &str, name: &str) -> BleCandidate {
        BleCandidate::new(id, Some(name.to_string()))
    }

    fn unnamed(id: &str) -> BleCandidate {
        BleCandidate::new(id, None)
    }

    #[test]
    fn exact_requires_full_name_or_id() {
        let c = named("uuid-1", "B1-Kitchen");
        assert!(score_ble_candidate("b1", &c, BleMatchMode::Exact).is_none());
        assert_eq!(
            score_ble_candidate("B1-Kitchen", &c, BleMatchMode::Exact),
            Some(900)
        );
        assert_eq!(
            score_ble_candidate("uuid-1", &c, BleMatchMode::Exact),
            Some(1000)
        );
        // Case-insensitive name
        assert_eq!(
            score_ble_candidate("b1-kitchen", &c, BleMatchMode::Exact),
            Some(900)
        );
    }

    #[test]
    fn fuzzy_allows_substring_but_not_printer_bonus_alone() {
        let c = named("uuid-1", "B1-Kitchen");
        assert!(score_ble_candidate("b1", &c, BleMatchMode::Fuzzy).is_some());
        // Printer-like name without selector hit
        assert!(score_ble_candidate("xyz", &c, BleMatchMode::Fuzzy).is_none());
    }

    #[test]
    fn exact_rejects_substring_that_fuzzy_accepts() {
        let cands = [named("id-a", "B1-Kitchen")];
        assert!(select_ble_candidate("B1", &cands, BleMatchMode::Exact).is_err());
        assert!(select_ble_candidate("B1", &cands, BleMatchMode::Fuzzy).is_ok());
    }

    #[test]
    fn ambiguous_fuzzy_errors() {
        let cands = [named("id-a", "B1-One"), named("id-b", "B1-Two")];
        let err = select_ble_candidate("B1", &cands, BleMatchMode::Fuzzy).unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "{err}");
    }

    #[test]
    fn exact_picks_unique_name() {
        let cands = [named("id-a", "B1-One"), named("id-b", "B1-Two")];
        let win = select_ble_candidate("B1-Two", &cands, BleMatchMode::Exact).expect("exact");
        assert_eq!(win.id, "id-b");
        assert_eq!(win.name.as_deref(), Some("B1-Two"));
    }

    #[test]
    fn unnamed_device_matches_only_by_id() {
        let cands = [unnamed("id-a")];
        // The old "(no name)" sentinel was compared as if it were a real name.
        assert!(select_ble_candidate("(no name)", &cands, BleMatchMode::Fuzzy).is_err());
        assert!(select_ble_candidate("id-a", &cands, BleMatchMode::Exact).is_ok());
    }

    #[test]
    fn blank_advertising_name_is_normalized_to_none() {
        assert_eq!(BleCandidate::new("id", Some("   ".into())).name, None);
        assert_eq!(BleCandidate::new("id", Some(String::new())).name, None);
    }

    #[test]
    fn printer_name_heuristic_is_shared() {
        for n in ["B1-Kitchen", "b21-x", "D110", "NIIMBOT-x", "jc-200"] {
            assert!(name_looks_like_label_printer(n), "{n}");
        }
        for n in ["AirPods", "random-watch", ""] {
            assert!(!name_looks_like_label_printer(n), "{n}");
        }
    }
}

// ─── USB serial ─────────────────────────────────────────────────────────────

#[cfg(feature = "serial")]
mod serial {
    use super::*;
    use std::io::{Read, Write};
    use tokio::time::sleep;

    pub struct SerialTransport {
        port: Box<dyn serialport::SerialPort>,
        rx_buf: Vec<u8>,
    }

    impl SerialTransport {
        pub fn open(path: &str) -> Result<Self> {
            let port = serialport::new(path, 115_200)
                .timeout(Duration::from_millis(200))
                .open()
                .map_err(|e| Error::transport(format!("open serial port {path}: {e}")))?;
            Ok(Self {
                port,
                rx_buf: Vec::new(),
            })
        }

        pub fn list_ports() -> Result<Vec<String>> {
            let ports = serialport::available_ports()
                .map_err(|e| Error::transport(format!("list serial ports: {e}")))?;
            Ok(ports.into_iter().map(|p| p.port_name).collect())
        }
    }

    impl Transport for SerialTransport {
        async fn send_raw(&mut self, data: &[u8]) -> Result<()> {
            self.port
                .write_all(data)
                .map_err(|e| Error::transport(format!("serial write: {e}")))?;
            self.port.flush().ok();
            Ok(())
        }

        async fn recv_packets(&mut self, wait: Duration) -> Result<Vec<Packet>> {
            let start = std::time::Instant::now();
            loop {
                let mut tmp = [0u8; 1024];
                match self.port.read(&mut tmp) {
                    Ok(n) if n > 0 => {
                        debug!(bytes = %hex::encode(&tmp[..n]), "RX serial");
                        self.rx_buf.extend_from_slice(&tmp[..n]);
                        let packets = Packet::drain_buffer(&mut self.rx_buf);
                        if !packets.is_empty() {
                            return Ok(packets);
                        }
                    }
                    Ok(_) | Err(_) => {
                        if start.elapsed() >= wait {
                            return Ok(Packet::drain_buffer(&mut self.rx_buf));
                        }
                        sleep(Duration::from_millis(20)).await;
                    }
                }
                if start.elapsed() >= wait {
                    return Ok(Packet::drain_buffer(&mut self.rx_buf));
                }
            }
        }
    }
}

#[cfg(feature = "serial")]
pub use serial::SerialTransport;
