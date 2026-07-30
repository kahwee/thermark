//! Transports: Bluetooth LE (macOS-friendly) and USB serial.
//!
//! Enable with Cargo features:
//! - `ble` — [`BleTransport`] (btleplug)
//! - `serial` — [`SerialTransport`]

use crate::errors::{Error, Result};
use crate::packet::Packet;
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

/// Score how well `selector` matches a scanned peripheral. Higher is better.
///
/// Returns `None` if this mode does not treat the device as a match.
pub fn score_ble_candidate(
    selector: &str,
    id: &str,
    name: &str,
    mode: BleMatchMode,
) -> Option<i32> {
    let sel = selector.to_ascii_lowercase();
    if sel.is_empty() {
        return None;
    }
    let id_l = id.to_ascii_lowercase();
    let name_l = name.to_ascii_lowercase();

    match mode {
        BleMatchMode::Exact => {
            if id_l == sel {
                return Some(1000);
            }
            if !name.is_empty() && name != "(no name)" && name_l == sel {
                return Some(900);
            }
            None
        }
        BleMatchMode::Fuzzy => {
            let mut score = 0i32;
            if id_l == sel {
                score += 1000;
            }
            if !name.is_empty() && name != "(no name)" && name_l == sel {
                score += 900;
            }
            if !name.is_empty() && name != "(no name)" && name_l.contains(&sel) {
                score += 100;
            }
            // Partial UUID/id only when selector is long enough to be intentional.
            if id_l.contains(&sel) && sel.len() >= 8 {
                score += 50;
            }
            if looks_like_label_printer(&name_l) {
                score += 30;
            }
            // Require a real selector hit (not printer-name bonus alone).
            if score < 100 { None } else { Some(score) }
        }
    }
}

fn looks_like_label_printer(name_l: &str) -> bool {
    name_l.starts_with("b1")
        || name_l.contains("b1-")
        || name_l.starts_with("b21")
        || name_l.contains("b21")
        || name_l.starts_with("d11")
        || name_l.contains("d110")
        || name_l.contains("niim")
        || name_l.starts_with("jc-")
}

/// Choose the best match among `(id, name)` candidates.
///
/// Fails when nothing matches, or when two candidates share the top score
/// (ambiguous fuzzy match).
pub fn select_ble_candidate(
    selector: &str,
    candidates: &[(String, String)],
    mode: BleMatchMode,
) -> std::result::Result<(String, String), String> {
    let mut ranked: Vec<(i32, String, String)> = candidates
        .iter()
        .filter_map(|(id, name)| {
            score_ble_candidate(selector, id, name, mode).map(|s| (s, id.clone(), name.clone()))
        })
        .collect();

    if ranked.is_empty() {
        return Err(match mode {
            BleMatchMode::Exact => format!(
                "no BLE device with exact name or id '{selector}'. Run `thermark scan` first \
                 (on macOS the id is a UUID, not a classic MAC). \
                 Tip: use the full advertising name, e.g. -a \"B1-YourPrinter\". \
                 Substring match: add --fuzzy (can pick the wrong device)."
            ),
            BleMatchMode::Fuzzy => format!(
                "no BLE device matching '{selector}' (fuzzy). Run `thermark scan` first \
                 (on macOS the id is a UUID, not a classic MAC). \
                 Tip: use the full name e.g. -a \"B1-YourPrinter\"."
            ),
        });
    }

    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let best = ranked[0].0;
    let ties: Vec<_> = ranked.iter().filter(|(s, _, _)| *s == best).collect();
    if ties.len() > 1 {
        let list = ties
            .iter()
            .map(|(_, id, name)| {
                if name.is_empty() {
                    id.clone()
                } else {
                    format!("{name} ({id})")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "ambiguous BLE match for '{selector}' (score {best}): {list}. \
             Use the full unique name or peripheral id."
        ));
    }

    Ok((ranked[0].1.clone(), ranked[0].2.clone()))
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
        rx: mpsc::UnboundedReceiver<Vec<u8>>,
        rx_buf: Vec<u8>,
        notify_task: Option<tokio::task::JoinHandle<()>>,
        /// True after explicit or Drop-time teardown (avoids double disconnect).
        closed: bool,
    }

    #[derive(Debug, Clone)]
    pub struct BleDeviceInfo {
        pub id: String,
        pub name: String,
        pub rssi: Option<i16>,
    }

    impl BleTransport {
        /// Scan for B1-class label-printer peripherals.
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
            let mut out = Vec::new();
            for p in peris {
                let props = match p
                    .properties()
                    .await
                    .map_err(|e| Error::transport(format!("BLE properties: {e}")))?
                {
                    Some(pr) => pr,
                    None => continue,
                };
                let name = props.local_name.unwrap_or_default();
                let name_l = name.to_ascii_lowercase();
                let interesting = name_l.contains("niim")
                    || name_l.starts_with("b1")
                    || name_l.contains("b1-")
                    || name_l.starts_with("b21")
                    || name_l.contains("b21")
                    || name_l.starts_with("d11")
                    || name_l.contains("d110")
                    || name_l.starts_with("jc-")
                    || props.services.contains(&PRINTER_SERVICE);

                if interesting {
                    out.push(BleDeviceInfo {
                        id: p.id().to_string(),
                        name: if name.is_empty() {
                            "(no name)".into()
                        } else {
                            name
                        },
                        rssi: props.rssi,
                    });
                }
            }
            adapter.stop_scan().await.ok();
            let mut map = HashMap::new();
            for d in out {
                map.insert(d.id.clone(), d);
            }
            let mut list: Vec<_> = map.into_values().collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
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

            let mut catalog: Vec<(String, String, Peripheral)> = Vec::new();
            for p in peris {
                let id = p.id().to_string();
                let props = p
                    .properties()
                    .await
                    .map_err(|e| Error::transport(format!("BLE properties: {e}")))?;
                let name = props
                    .as_ref()
                    .and_then(|pr| pr.local_name.clone())
                    .unwrap_or_default();
                catalog.push((id, name, p));
            }

            let pairs: Vec<(String, String)> = catalog
                .iter()
                .map(|(id, name, _)| (id.clone(), name.clone()))
                .collect();
            let (win_id, win_name) =
                select_ble_candidate(selector, &pairs, mode).map_err(Error::transport)?;

            let peripheral = catalog
                .into_iter()
                .find(|(id, _, _)| id == &win_id)
                .map(|(_, _, p)| p)
                .ok_or_else(|| Error::transport("internal: matched BLE device vanished"))?;
            let name = win_name;

            info!(
                name = %if name.is_empty() { "(unknown)" } else { name.as_str() },
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
        fn drop(&mut self) {
            if self.closed {
                return;
            }
            self.closed = true;
            if let Some(task) = self.notify_task.take() {
                task.abort();
            }
            // Prefer async disconnect on the current runtime so mid-error paths
            // (e.g. print failure without Session::finish) still free the link.
            let peripheral = self.peripheral.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = peripheral.disconnect().await;
                });
            }
        }
    }

    impl Transport for BleTransport {
        async fn send_raw(&mut self, data: &[u8]) -> Result<()> {
            const CHUNK: usize = 180;
            for chunk in data.chunks(CHUNK) {
                self.peripheral
                    .write(&self.characteristic, chunk, WriteType::WithoutResponse)
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

    #[test]
    fn exact_requires_full_name_or_id() {
        assert!(score_ble_candidate("b1", "uuid-1", "B1-Kitchen", BleMatchMode::Exact).is_none());
        assert_eq!(
            score_ble_candidate("B1-Kitchen", "uuid-1", "B1-Kitchen", BleMatchMode::Exact),
            Some(900)
        );
        assert_eq!(
            score_ble_candidate("uuid-1", "uuid-1", "B1-Kitchen", BleMatchMode::Exact),
            Some(1000)
        );
        // Case-insensitive name
        assert_eq!(
            score_ble_candidate("b1-kitchen", "uuid-1", "B1-Kitchen", BleMatchMode::Exact),
            Some(900)
        );
    }

    #[test]
    fn fuzzy_allows_substring_but_not_printer_bonus_alone() {
        assert!(score_ble_candidate("b1", "uuid-1", "B1-Kitchen", BleMatchMode::Fuzzy).is_some());
        // Printer-like name without selector hit
        assert!(score_ble_candidate("xyz", "uuid-1", "B1-Kitchen", BleMatchMode::Fuzzy).is_none());
    }

    #[test]
    fn exact_rejects_substring_that_fuzzy_accepts() {
        let cands = [("id-a".to_string(), "B1-Kitchen".to_string())];
        assert!(select_ble_candidate("B1", &cands, BleMatchMode::Exact).is_err());
        assert!(select_ble_candidate("B1", &cands, BleMatchMode::Fuzzy).is_ok());
    }

    #[test]
    fn ambiguous_fuzzy_errors() {
        let cands = [
            ("id-a".to_string(), "B1-One".to_string()),
            ("id-b".to_string(), "B1-Two".to_string()),
        ];
        let err = select_ble_candidate("B1", &cands, BleMatchMode::Fuzzy).unwrap_err();
        assert!(err.contains("ambiguous"), "{err}");
    }

    #[test]
    fn exact_picks_unique_name() {
        let cands = [
            ("id-a".to_string(), "B1-One".to_string()),
            ("id-b".to_string(), "B1-Two".to_string()),
        ];
        let (id, name) =
            select_ble_candidate("B1-Two", &cands, BleMatchMode::Exact).expect("exact");
        assert_eq!(id, "id-b");
        assert_eq!(name, "B1-Two");
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
