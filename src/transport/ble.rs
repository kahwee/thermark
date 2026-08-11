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

pub struct BleTransport {
    peripheral: Peripheral,
    characteristic: Characteristic,
    /// Acknowledged when the characteristic allows it — see
    /// [`choose_write_type`].
    write_type: WriteType,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    notify_task: Option<tokio::task::JoinHandle<()>>,
    /// True after explicit or Drop-time teardown (avoids double disconnect).
    closed: bool,
}

/// Owns cleanup from the moment a peripheral connects until the complete
/// transport has been constructed. This also covers future cancellation.
struct BleSetupGuard(Option<Peripheral>);

impl BleSetupGuard {
    fn new(peripheral: &Peripheral) -> Self {
        Self(Some(peripheral.clone()))
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for BleSetupGuard {
    fn drop(&mut self) {
        if let Some(peripheral) = self.0.take() {
            disconnect_on_drop(peripheral, "BLE disconnected after incomplete setup");
        }
    }
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
            let interesting =
                candidate.looks_like_label_printer() || props.services.contains(&PRINTER_SERVICE);
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
        let mut setup_guard = BleSetupGuard::new(&peripheral);
        if let Err(e) = peripheral.discover_services().await {
            let error = Error::transport(format!(
                "discover services failed: {e}. Wrong device or incomplete BLE connection — \
                 run `thermark scan` and pass -a with the full advertising name."
            ));
            return Err(error);
        }

        let characteristic = match find_printer_char(&peripheral).await {
            Ok(characteristic) => characteristic,
            Err(error) => return Err(error),
        };

        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        if let Err(e) = peripheral.subscribe(&characteristic).await {
            return Err(Error::transport(format!("subscribe: {e}")));
        }

        let mut notif = match peripheral.notifications().await {
            Ok(notifications) => notifications,
            Err(e) => return Err(Error::transport(format!("notifications stream: {e}"))),
        };
        let notify_task = tokio::spawn(async move {
            while let Some(n) = notif.next().await {
                let _ = tx.send(n.value);
            }
        });

        let transport = Self {
            peripheral,
            write_type: choose_write_type(&characteristic),
            characteristic,
            rx,
            notify_task: Some(notify_task),
            closed: false,
        };
        setup_guard.disarm();
        sleep(Duration::from_millis(200)).await;
        Ok(transport)
    }

    async fn close_inner(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        if let Some(task) = self.notify_task.take() {
            task.abort();
        }
        self.peripheral
            .disconnect()
            .await
            .map_err(|e| Error::transport(format!("BLE disconnect: {e}")))?;
        self.closed = true;
        debug!("BLE disconnected");
        Ok(())
    }
}

impl Drop for BleTransport {
    /// Backstop teardown for paths that skip [`Transport::close`]
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

        disconnect_on_drop(self.peripheral.clone(), "BLE disconnected on drop");
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

    async fn recv_raw(&mut self, wait: Duration) -> Result<Vec<u8>> {
        if let Ok(first) = self.rx.try_recv() {
            let mut bytes = first;
            while let Ok(chunk) = self.rx.try_recv() {
                bytes.extend_from_slice(&chunk);
            }
            debug!(bytes = %hex::encode(&bytes), "RX bytes");
            return Ok(bytes);
        }
        match timeout(wait, self.rx.recv()).await {
            Ok(Some(chunk)) => {
                let mut bytes = chunk;
                while let Ok(chunk) = self.rx.try_recv() {
                    bytes.extend_from_slice(&chunk);
                }
                debug!(bytes = %hex::encode(&bytes), "RX bytes");
                Ok(bytes)
            }
            Ok(None) => Err(Error::transport("BLE notification channel closed")),
            Err(_) => Ok(Vec::new()),
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.close_inner().await
    }
}

/// Write type for row data.
///
/// Unacknowledged, with a fixed inter-packet delay — what this protocol
/// expects for row streaming. Acknowledged writes were tried against a real
/// B1 and made no difference to the truncation being investigated, so the
/// deviation was not worth keeping.
fn choose_write_type(_characteristic: &Characteristic) -> WriteType {
    WriteType::WithoutResponse
}

fn disconnect_on_drop(peripheral: Peripheral, success_message: &'static str) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        debug!("BLE connection dropped outside a runtime");
        return;
    };
    match handle.runtime_flavor() {
        RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| {
                handle.block_on(async {
                    match peripheral.disconnect().await {
                        Ok(()) => debug!("{success_message}"),
                        Err(e) => debug!(error = %e, "BLE disconnect on drop (ignored)"),
                    }
                });
            });
        }
        _ => {
            handle.spawn(async move {
                let _ = peripheral.disconnect().await;
            });
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
