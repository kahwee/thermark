//! Transports: Bluetooth LE (macOS-friendly) and USB serial.
//!
//! Enable with Cargo features:
//! - `ble` — [`BleTransport`] (btleplug)
//! - `serial` — [`SerialTransport`]

use crate::errors::{Error, Result};
use crate::packet::Packet;
use async_trait::async_trait;
use tracing::debug;
use std::time::Duration;

/// Common packet transport used by [`crate::printer::PrinterClient`].
#[async_trait]
pub trait Transport: Send {
    async fn send_raw(&mut self, data: &[u8]) -> Result<()>;
    async fn recv_packets(&mut self, wait: Duration) -> Result<Vec<Packet>>;

    async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        let bytes = packet.encode();
        debug!(bytes = %hex::encode(&bytes), "TX");
        self.send_raw(&bytes).await
    }
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
    use tracing::{debug, info};
    use std::collections::HashMap;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, timeout};
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
        _notify_task: tokio::task::JoinHandle<()>,
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

        /// Connect by peripheral id string or by name substring (case-insensitive).
        pub async fn connect(selector: &str, scan_for: Duration) -> Result<Self> {
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
            let sel = selector.to_ascii_lowercase();

            let mut best: Option<(i32, Peripheral, String)> = None;
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
                let id_l = id.to_ascii_lowercase();
                let name_l = name.to_ascii_lowercase();

                let mut score = 0i32;
                if id_l == sel {
                    score += 1000;
                }
                if !name.is_empty() && name_l == sel {
                    score += 900;
                }
                if !name.is_empty() && name_l.contains(&sel) {
                    score += 100;
                }
                if id_l.contains(&sel) && sel.len() >= 8 {
                    score += 50;
                }
                if name_l.starts_with("b1")
                    || name_l.contains("b1-")
                    || name_l.starts_with("b21")
                    || name_l.contains("b21")
                    || name_l.starts_with("d11")
                    || name_l.contains("d110")
                    || name_l.contains("niim")
                    || name_l.starts_with("jc-")
                {
                    score += 30;
                }
                if score < 100 {
                    continue;
                }
                if best.as_ref().map(|(s, _, _)| score > *s).unwrap_or(true) {
                    best = Some((score, p, name));
                }
            }

            let (_score, peripheral, name) = best.ok_or_else(|| {
                Error::transport(format!(
                    "no BLE device matching '{selector}'. Run `thermark scan` first \
                     (on macOS the id is a UUID, not a classic MAC). \
                     Tip: use the full name e.g. -a \"B1-YourPrinter\"."
                ))
            })?;

            info!(
                name = %if name.is_empty() { "(unknown)" } else { name.as_str() },
                id = %peripheral.id(),
                "connecting"
            );

            peripheral
                .connect()
                .await
                .map_err(|e| Error::transport(format!("BLE connect: {e}")))?;
            peripheral
                .discover_services()
                .await
                .map_err(|e| Error::transport(format!("discover services: {e}")))?;

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
                _notify_task: notify_task,
            })
        }

        pub async fn disconnect(self) -> Result<()> {
            self.peripheral.disconnect().await.ok();
            Ok(())
        }
    }

    #[async_trait]
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
        adapters.into_iter().next().ok_or_else(|| {
            Error::transport("no Bluetooth adapter found — is Bluetooth enabled?")
        })
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
    bluetooth_available, BleDeviceInfo, BleTransport, NIIMBOT_CHAR, NIIMBOT_SERVICE, PRINTER_CHAR,
    PRINTER_SERVICE,
};

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

    #[async_trait]
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
