//! Transports: Bluetooth LE (macOS-friendly) and USB serial.

use crate::packet::Packet;
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::stream::StreamExt;
use log::{debug, info};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// BLE service / characteristic used by common pocket thermal label printers
/// (community reverse-engineering; same UUIDs as NIIMBOT-class devices).
pub const PRINTER_SERVICE: Uuid = Uuid::from_u128(0xe7810a71_73ae_499d_8c15_faa9aef0c3f2);
pub const PRINTER_CHAR: Uuid = Uuid::from_u128(0xbef8d6c9_9c21_4c9e_b632_bd58c1009f9f);
/// Back-compat aliases
pub const NIIMBOT_SERVICE: Uuid = PRINTER_SERVICE;
pub const NIIMBOT_CHAR: Uuid = PRINTER_CHAR;

#[async_trait]
pub trait Transport: Send {
    async fn send_raw(&mut self, data: &[u8]) -> Result<()>;
    async fn recv_packets(&mut self, wait: Duration) -> Result<Vec<Packet>>;

    async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        let bytes = packet.encode();
        debug!("TX {}", hex::encode(&bytes));
        self.send_raw(&bytes).await
    }
}

// ─── BLE ────────────────────────────────────────────────────────────────────

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
    /// Scan for NIIMBOT-like peripherals.
    pub async fn scan(duration: Duration) -> Result<Vec<BleDeviceInfo>> {
        let adapter = default_adapter().await?;
        adapter.start_scan(ScanFilter::default()).await?;
        sleep(duration).await;
        let peris = adapter.peripherals().await?;
        let mut out = Vec::new();
        for p in peris {
            let props = match p.properties().await? {
                Some(pr) => pr,
                None => continue,
            };
            let name = props.local_name.unwrap_or_default();
            let name_l = name.to_ascii_lowercase();
            // Match common advertising names; also keep empty if services match later.
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
        // Dedupe by id
        let mut map = HashMap::new();
        for d in out {
            map.insert(d.id.clone(), d);
        }
        let mut list: Vec<_> = map.into_values().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }

    /// Connect by peripheral id string or by name substring (case-insensitive).
    ///
    /// Prefers devices whose name looks like a NIIMBOT printer (B1, D110, …).
    pub async fn connect(selector: &str, scan_for: Duration) -> Result<Self> {
        let adapter = default_adapter().await?;
        adapter.start_scan(ScanFilter::default()).await?;
        sleep(scan_for).await;
        adapter.stop_scan().await.ok();

        let peris = adapter.peripherals().await?;
        let sel = selector.to_ascii_lowercase();

        // Score candidates; higher is better.
        let mut best: Option<(i32, Peripheral, String)> = None;
        for p in peris {
            let id = p.id().to_string();
            let props = p.properties().await?;
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
                score += 50; // partial UUID only if selector is long
            }
            // Prefer real printer advertising names (avoid hex IDs like "7c3b1d98"
            // which contain the letters "b1" by accident).
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
            // Avoid matching tiny accidental substrings in unrelated names
            if score < 100 {
                continue;
            }
            if best.as_ref().map(|(s, _, _)| score > *s).unwrap_or(true) {
                best = Some((score, p, name));
            }
        }

        let (_score, peripheral, name) = best.ok_or_else(|| {
            anyhow!(
                "no BLE device matching '{selector}'. Run `thermark scan` first \
                 (on macOS the id is a UUID, not a classic MAC). \
                 Tip: use the full name e.g. -a \"B1-YourPrinter\"."
            )
        })?;

        info!(
            "connecting to {} ({})",
            if name.is_empty() {
                "(unknown)"
            } else {
                name.as_str()
            },
            peripheral.id()
        );

        peripheral.connect().await.context("BLE connect")?;
        peripheral
            .discover_services()
            .await
            .context("discover services")?;

        let characteristic = find_printer_char(&peripheral).await?;

        // Notifications → channel
        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        peripheral.subscribe(&characteristic).await.context("subscribe")?;

        let mut notif = peripheral
            .notifications()
            .await
            .context("notifications stream")?;
        let notify_task = tokio::spawn(async move {
            while let Some(n) = notif.next().await {
                let _ = tx.send(n.value);
            }
        });

        // Small settle delay after subscribe
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
        // BLE ATT MTU can be small; chunk conservatively.
        const CHUNK: usize = 180;
        for chunk in data.chunks(CHUNK) {
            self.peripheral
                .write(&self.characteristic, chunk, WriteType::WithoutResponse)
                .await
                .context("BLE write")?;
            // Give the radio a breath between chunks
            sleep(Duration::from_millis(5)).await;
        }
        Ok(())
    }

    async fn recv_packets(&mut self, wait: Duration) -> Result<Vec<Packet>> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            // Drain whatever is already queued
            while let Ok(chunk) = self.rx.try_recv() {
                debug!("RX chunk {}", hex::encode(&chunk));
                self.rx_buf.extend_from_slice(&chunk);
            }
            let packets = Packet::drain_buffer(&mut self.rx_buf);
            if !packets.is_empty() {
                for p in &packets {
                    debug!("RX pkt cmd={:#04x} data={}", p.cmd, hex::encode(&p.data));
                }
                return Ok(packets);
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Ok(vec![]);
            }
            match timeout(remaining, self.rx.recv()).await {
                Ok(Some(chunk)) => {
                    debug!("RX chunk {}", hex::encode(&chunk));
                    self.rx_buf.extend_from_slice(&chunk);
                }
                Ok(None) => bail!("BLE notification channel closed"),
                Err(_) => return Ok(vec![]),
            }
        }
    }
}

async fn default_adapter() -> Result<Adapter> {
    let manager = Manager::new().await.context("btleplug Manager")?;
    let adapters = manager.adapters().await.context("list adapters")?;
    adapters
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no Bluetooth adapter found — is Bluetooth enabled?"))
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

    // Prefer known UUID
    if let Some(c) = chars.iter().find(|c| c.uuid == PRINTER_CHAR) {
        return Ok(c.clone());
    }

    // No silent fallback to random GATT devices — that caused connecting to
    // unrelated BLE gadgets when the name selector was too loose.
    bail!(
        "printer characteristic {PRINTER_CHAR} not found (wrong device?). \
         Discovered characteristics:\n{}",
        chars
            .iter()
            .map(|c| format!("  {} props={:?}", c.uuid, c.properties))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

// ─── USB serial ─────────────────────────────────────────────────────────────

pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
    rx_buf: Vec<u8>,
}

impl SerialTransport {
    pub fn open(path: &str) -> Result<Self> {
        let port = serialport::new(path, 115_200)
            .timeout(Duration::from_millis(200))
            .open()
            .with_context(|| format!("open serial port {path}"))?;
        Ok(Self {
            port,
            rx_buf: Vec::new(),
        })
    }

    pub fn list_ports() -> Result<Vec<String>> {
        let ports = serialport::available_ports().context("list serial ports")?;
        Ok(ports.into_iter().map(|p| p.port_name).collect())
    }
}

#[async_trait]
impl Transport for SerialTransport {
    async fn send_raw(&mut self, data: &[u8]) -> Result<()> {
        self.port.write_all(data).context("serial write")?;
        self.port.flush().ok();
        Ok(())
    }

    async fn recv_packets(&mut self, wait: Duration) -> Result<Vec<Packet>> {
        let start = std::time::Instant::now();
        loop {
            let mut tmp = [0u8; 1024];
            match self.port.read(&mut tmp) {
                Ok(n) if n > 0 => {
                    debug!("RX serial {}", hex::encode(&tmp[..n]));
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

// serialport::SerialPort::write_all is on std::io::Write
use std::io::{Read, Write};
