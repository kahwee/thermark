//! Offline protocol replay and deterministic fault injection. These are not hardware evidence.
use crate::{Error, Result, Transport};
use serde::Deserialize;
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayStep {
    pub tx: String,
    pub rx: Vec<String>,
}
/// Exact TX matching with fragmented RX. Fixtures must contain public demo data only.
type ReplayFrame = (Vec<u8>, Vec<Vec<u8>>);
pub struct ReplayTransport {
    steps: VecDeque<ReplayFrame>,
    received: VecDeque<Vec<u8>>,
    pub writes: usize,
    pub closed: bool,
}
impl ReplayTransport {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let steps: Vec<ReplayStep> =
            serde_json::from_slice(bytes).map_err(|e| Error::msg(format!("replay JSON: {e}")))?;
        let mut decoded = VecDeque::new();
        for step in steps {
            let tx = hex::decode(step.tx).map_err(|e| Error::msg(format!("replay hex: {e}")))?;
            let rx = step
                .rx
                .into_iter()
                .map(|s| hex::decode(s).map_err(|e| Error::msg(format!("replay hex: {e}"))))
                .collect::<Result<_>>()?;
            decoded.push_back((tx, rx));
        }
        Ok(Self {
            steps: decoded,
            received: VecDeque::new(),
            writes: 0,
            closed: false,
        })
    }
    pub fn is_complete(&self) -> bool {
        self.steps.is_empty() && self.received.is_empty()
    }
}
impl Transport for ReplayTransport {
    async fn send_raw(&mut self, bytes: &[u8]) -> Result<()> {
        let (expected, replies) = self
            .steps
            .front()
            .ok_or_else(|| Error::transport("unexpected replay write"))?;
        if expected != bytes {
            return Err(Error::transport("replay write differs from fixture"));
        }
        self.received.extend(replies.iter().cloned());
        self.steps.pop_front();
        self.writes += 1;
        Ok(())
    }
    async fn recv_raw(&mut self, wait: Duration) -> Result<Vec<u8>> {
        if let Some(bytes) = self.received.pop_front() {
            return Ok(bytes);
        }
        tokio::time::sleep(wait).await;
        Ok(Vec::new())
    }
    async fn close(&mut self) -> Result<()> {
        self.closed = true;
        Ok(())
    }
}

/// Fail after a command prefix may have reached hardware, or delay replies within read budgets.
/// Accepted prefixes are recorded separately because the underlying mock requires full frames.
pub struct FaultTransport<T> {
    pub inner: T,
    pub fail_command: Option<u8>,
    pub accepted_prefix: usize,
    pub receive_delay: Duration,
    pub writes: Vec<Vec<u8>>,
    pub closed: bool,
}
impl<T> FaultTransport<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            fail_command: None,
            accepted_prefix: 0,
            receive_delay: Duration::ZERO,
            writes: Vec::new(),
            closed: false,
        }
    }
}
impl<T: Transport> Transport for FaultTransport<T> {
    async fn send_raw(&mut self, bytes: &[u8]) -> Result<()> {
        if self
            .fail_command
            .is_some_and(|command| bytes.get(2) == Some(&command))
        {
            self.writes
                .push(bytes[..self.accepted_prefix.min(bytes.len())].to_vec());
            return Err(Error::transport(
                "injected disconnect after potentially accepted prefix",
            ));
        }
        self.writes.push(bytes.to_vec());
        self.inner.send_raw(bytes).await
    }
    async fn recv_raw(&mut self, wait: Duration) -> Result<Vec<u8>> {
        let pause = self.receive_delay.min(wait);
        if !pause.is_zero() {
            tokio::time::sleep(pause).await;
            self.receive_delay -= pause;
        }
        if pause == wait {
            return Ok(Vec::new());
        }
        self.inner.recv_raw(wait - pause).await
    }
    async fn close(&mut self) -> Result<()> {
        self.closed = true;
        self.inner.close().await
    }
}
