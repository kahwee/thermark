//! Asynchronous USB serial transport.

use super::Transport;
use crate::errors::{Error, Result};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tokio_serial::SerialPortBuilderExt;
use tracing::debug;

trait AsyncSerialIo: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T> AsyncSerialIo for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}

pub struct SerialTransport {
    port: Box<dyn AsyncSerialIo>,
}

impl SerialTransport {
    pub fn open(path: &str) -> Result<Self> {
        let port = tokio_serial::new(path, 115_200)
            .open_native_async()
            .map_err(|error| Error::transport(format!("open serial port {path}: {error}")))?;
        Ok(Self {
            port: Box::new(port),
        })
    }

    #[cfg(test)]
    fn from_stream(stream: impl AsyncSerialIo + 'static) -> Self {
        Self {
            port: Box::new(stream),
        }
    }

    pub fn list_ports() -> Result<Vec<String>> {
        let ports = serialport::available_ports()
            .map_err(|error| Error::transport(format!("list serial ports: {error}")))?;
        Ok(ports.into_iter().map(|port| port.port_name).collect())
    }
}

impl Transport for SerialTransport {
    async fn send_raw(&mut self, data: &[u8]) -> Result<()> {
        self.port
            .write_all(data)
            .await
            .map_err(|error| Error::transport(format!("serial write: {error}")))?;
        self.port
            .flush()
            .await
            .map_err(|error| Error::transport(format!("serial flush: {error}")))?;
        Ok(())
    }

    async fn recv_raw(&mut self, wait: Duration) -> Result<Vec<u8>> {
        let mut bytes = [0u8; 1024];
        match timeout(wait, self.port.read(&mut bytes)).await {
            Err(_) => Ok(Vec::new()),
            Ok(Ok(0)) => Err(Error::transport("serial connection closed")),
            Ok(Ok(count)) => {
                debug!(bytes = %hex::encode(&bytes[..count]), "RX serial");
                Ok(bytes[..count].to_vec())
            }
            Ok(Err(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(Vec::new())
            }
            Ok(Err(error)) => Err(Error::transport(format!("serial read: {error}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_partial_data_without_blocking_the_runtime() {
        let (client, mut peer) = tokio::io::duplex(64);
        let mut transport = SerialTransport::from_stream(client);
        peer.write_all(&[0x55]).await.unwrap();
        assert_eq!(
            transport.recv_raw(Duration::from_millis(20)).await.unwrap(),
            vec![0x55]
        );
    }

    #[tokio::test]
    async fn timeout_is_empty_data() {
        let (client, _peer) = tokio::io::duplex(64);
        let mut transport = SerialTransport::from_stream(client);
        assert!(
            transport
                .recv_raw(Duration::from_millis(1))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn disconnect_is_an_error() {
        let (client, peer) = tokio::io::duplex(64);
        drop(peer);
        let mut transport = SerialTransport::from_stream(client);
        let error = transport
            .recv_raw(Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("closed"));
    }

    #[tokio::test]
    async fn send_flushes_bytes() {
        let (client, mut peer) = tokio::io::duplex(64);
        let mut transport = SerialTransport::from_stream(client);
        transport.send_raw(&[1, 2, 3]).await.unwrap();
        let mut bytes = [0; 3];
        peer.read_exact(&mut bytes).await.unwrap();
        assert_eq!(bytes, [1, 2, 3]);
    }
}
