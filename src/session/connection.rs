use super::*;
/// Native transport sum type; the protocol client remains transport-agnostic.
pub enum AnyTransport {
    #[cfg(feature = "ble")]
    Ble(BleTransport),
    #[cfg(feature = "serial")]
    Usb(SerialTransport),
}

impl Transport for AnyTransport {
    #[allow(unused_variables)]
    async fn send_raw(&mut self, data: &[u8]) -> crate::Result<()> {
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
    async fn recv_raw(&mut self, wait: std::time::Duration) -> crate::Result<Vec<u8>> {
        match self {
            #[cfg(feature = "ble")]
            Self::Ble(transport) => transport.recv_raw(wait).await,
            #[cfg(feature = "serial")]
            Self::Usb(transport) => transport.recv_raw(wait).await,
            #[cfg(not(any(feature = "ble", feature = "serial")))]
            _ => unreachable!("AnyTransport has no variants"),
        }
    }

    async fn close(&mut self) -> crate::Result<()> {
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

impl Session<AnyTransport> {
    pub async fn connect(conn: &ConnectionOptions, target: PrintTarget) -> Result<Self> {
        Self::connect_with_identity(conn, target, IdentityDetail::Profile).await
    }

    /// Open a session and retain the full identity report for presentation.
    ///
    /// Normal printing only needs [`Self::connect`]; this variant keeps the
    /// firmware and hardware metadata queries used by the `identify` command.
    pub async fn connect_detailed(conn: &ConnectionOptions, target: PrintTarget) -> Result<Self> {
        Self::connect_with_identity(conn, target, IdentityDetail::Full).await
    }

    #[allow(unused_variables)]
    #[cfg_attr(
        not(any(feature = "ble", feature = "serial")),
        expect(clippy::unused_async, reason = "connecting awaits enabled transports")
    )]
    async fn connect_with_identity(
        conn: &ConnectionOptions,
        target: PrintTarget,
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
                    let client = PrinterClient::new_with_task(
                        AnyTransport::Ble(ble),
                        target.model,
                        target.task.initial_task(),
                    )
                    .with_pacing(conn.pacing);
                    Self::finish_connect(client, ConnPref::Ble, target, identity_detail).await
                }
            }
            ConnPref::Usb => {
                #[cfg(not(feature = "serial"))]
                bail!("this thermark binary was built without USB serial support");
                #[cfg(feature = "serial")]
                {
                    let ser = SerialTransport::open(&conn.addr)
                        .with_context(|| format!("open serial {}", conn.addr))?;
                    let client = PrinterClient::new_with_task(
                        AnyTransport::Usb(ser),
                        target.model,
                        target.task.initial_task(),
                    )
                    .with_pacing(conn.pacing);
                    Self::finish_connect(client, ConnPref::Usb, target, identity_detail).await
                }
            }
        }
    }
}
