//! Transports: Bluetooth LE (macOS-friendly) and USB serial.
//!
//! Enable with Cargo features:
//! - `ble` — [`BleTransport`] (btleplug)
//! - `serial` — [`SerialTransport`]

use crate::errors::{Error, Result};
use crate::packet::{MAX_FRAME_LEN, Packet};
use std::fmt;
use std::time::Duration;
use tracing::debug;

/// Common packet transport used by [`crate::printer::PrinterClient`].
///
/// Native `async fn` in traits (no `async-trait`); not object-safe by design.
pub trait Transport: Send {
    fn send_raw(&mut self, data: &[u8]) -> impl std::future::Future<Output = Result<()>> + Send;

    fn recv_raw(
        &mut self,
        wait: Duration,
    ) -> impl std::future::Future<Output = Result<Vec<u8>>> + Send;

    fn close(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    fn send_packet(
        &mut self,
        packet: &Packet,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        async {
            // A print can contain hundreds of row packets. Keep their small,
            // bounded frame storage inline instead of allocating a fresh Vec
            // for every call.
            let mut frame = [0u8; MAX_FRAME_LEN];
            let bytes = packet.encode_into(&mut frame)?;
            debug!(bytes = %hex::encode(bytes), "TX");
            self.send_raw(bytes).await
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

/// Whether a serial device path appears to belong to a configured printer
/// selector. macOS commonly exposes an already-connected BLE printer as a
/// `/dev/cu.<advertising-name>` endpoint.
pub fn serial_path_matches_selector(path: &str, selector: &str) -> bool {
    let path = path.trim().to_ascii_lowercase();
    let selector = selector.trim().to_ascii_lowercase();
    !selector.is_empty() && path.ends_with(&selector)
}

/// Find a serial endpoint that matches a BLE selector, when serial support is
/// available. This is primarily a macOS diagnostic: a matching endpoint can
/// mean another Bluetooth client already owns the printer's BLE session.
#[cfg(all(target_os = "macos", feature = "ble", feature = "serial"))]
pub(crate) fn serial_port_for_selector(selector: &str) -> Option<String> {
    serial::SerialTransport::list_ports()
        .ok()?
        .into_iter()
        .find(|path| serial_path_matches_selector(path, selector))
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
mod ble;

#[cfg(feature = "ble")]
pub use ble::{BleDeviceInfo, BleTransport, PRINTER_CHAR, PRINTER_SERVICE, bluetooth_available};

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
    fn serial_path_matches_configured_printer_name() {
        assert!(serial_path_matches_selector(
            "/dev/cu.B1-I304120661",
            "B1-I304120661"
        ));
        assert!(serial_path_matches_selector(
            "/dev/tty.b1-i304120661",
            "b1-i304120661"
        ));
        assert!(!serial_path_matches_selector(
            "/dev/cu.B1-OtherPrinter",
            "B1-I304120661"
        ));
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
mod serial;

#[cfg(feature = "serial")]
pub use serial::SerialTransport;
