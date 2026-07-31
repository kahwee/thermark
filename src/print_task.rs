//! Print-task variants (command sequences differ by model / firmware era).
//!
//! Aligns with the community wiki:
//! Print-task variants differ in the PrintStart and SetPageSize forms they use.
//!
//! **Hardware-tested in this repo:** only [`PrintTask::B1`] on a real B1.
//! Other tasks are implemented from public protocol notes and should be treated
//! as experimental until verified.

use crate::packet::Packet;
use crate::protocol::{self, Model};

/// Which on-wire print sequence to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum PrintTask {
    /// B1 / many 2024+ printers: 7-byte PrintStart, 6-byte SetPageSize, status poll.
    /// **Hardware-tested** (B1) in this project.
    #[default]
    #[value(name = "b1")]
    B1,
    /// Older B21-style: 1-byte PrintStart, 4-byte SetPageSize.
    /// Experimental — not hardware-verified here.
    #[value(name = "b21v1", alias = "b21", alias = "b21_v1")]
    B21V1,
    /// D110 / D11-style (203 dpi): simple start, 4-byte page size, quantity-ish flow simplified.
    /// Experimental — not hardware-verified here.
    #[value(name = "d110", alias = "d11")]
    D110,
    /// Plain 1-byte PrintStart + 4-byte page size (works on some firmwares).
    /// Experimental as a generic fallback.
    #[value(name = "simple", alias = "niimprint")]
    Simple,
}

impl PrintTask {
    /// Default task for a model family (best guess; override if prints fail).
    pub fn for_model(model: Model) -> Self {
        match model {
            Model::B1 => Self::B1,
            Model::B21 | Model::B18 => Self::B21V1,
            Model::D11 | Model::D110 => Self::D110,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "b1" => Some(Self::B1),
            "b21" | "b21v1" | "b21_v1" => Some(Self::B21V1),
            "d11" | "d110" => Some(Self::D110),
            "simple" | "niimprint" => Some(Self::Simple),
            _ => None,
        }
    }

    /// True only for tasks we have actually run on physical hardware in this repo.
    pub fn hardware_tested(self) -> bool {
        matches!(self, Self::B1)
    }

    pub fn notes(self) -> &'static str {
        match self {
            Self::B1 => "Hardware-tested on B1. 7-byte PrintStart, 6-byte page size, status poll.",
            Self::B21V1 => {
                "Experimental. Based on wiki B21_V1 (1-byte PrintStart, 4-byte page size)."
            }
            Self::D110 => "Experimental. Based on wiki D110 / D11 203dpi notes.",
            Self::Simple => "Experimental fallback (plain 1-byte PrintStart).",
        }
    }

    /// Width this sequence can address. See [`effective_max_width_px`] for the
    /// limit that actually applies to a job.
    pub fn max_width_px(self) -> u32 {
        match self {
            Self::B1 | Self::B21V1 | Self::Simple => crate::geometry::HEAD_WIDE_PX,
            Self::D110 => crate::geometry::HEAD_NARROW_PX,
        }
    }

    /// Build PrintStart for this task.
    pub fn print_start(self, total_pages: u16) -> Packet {
        match self {
            Self::B1 => {
                let mut data = Vec::with_capacity(7);
                data.extend_from_slice(&total_pages.to_be_bytes());
                data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00]); // pad + pageColor
                protocol::pkt(protocol::Cmd::PrintStart, data)
            }
            Self::B21V1 | Self::D110 | Self::Simple => protocol::print_start_simple(),
        }
    }

    /// Build SetPageSize for this task.
    pub fn set_page_size(self, rows: u16, cols: u16, copies: u16) -> Packet {
        match self {
            Self::B1 => protocol::set_page_size_b1(rows, cols, copies),
            Self::B21V1 | Self::D110 | Self::Simple => protocol::set_page_size(rows, cols),
        }
    }

    /// Whether to send PrintClear before PageStart (D11_V1 / D110 wiki).
    pub fn uses_print_clear(self) -> bool {
        matches!(self, Self::D110)
    }

    /// Status-poll iterations before PrintEnd (B1 wiki).
    pub fn status_polls(self) -> u32 {
        match self {
            Self::B1 => 8,
            Self::B21V1 | Self::D110 | Self::Simple => 4,
        }
    }
}

impl PrintTask {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::B1 => "b1",
            Self::B21V1 => "b21v1",
            Self::D110 => "d110",
            Self::Simple => "simple",
        }
    }
}

impl std::fmt::Display for PrintTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `f.pad` honours width/alignment; `write!(f, "…")` silently ignores
        // them, which left the `thermark tasks` table columns ragged.
        f.pad(self.as_str())
    }
}

impl std::str::FromStr for PrintTask {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
            .ok_or_else(|| format!("unknown print task '{s}' (try b1, b21v1, d110, simple)"))
    }
}

/// The width limit that actually applies to a job: the narrower of the model's
/// printhead and the print sequence's addressable width.
///
/// Use this everywhere a raster is sized or checked. Sizing a canvas from the
/// model alone (and only checking the pair later) means a mismatched
/// `--model`/`--task` encodes the whole image before being rejected.
pub fn effective_max_width_px(model: Model, task: PrintTask) -> u32 {
    model.max_width_px().min(task.max_width_px())
}

/// Honest hardware matrix for docs and `thermark tasks`.
pub fn hardware_matrix() -> &'static [HardwareSupport] {
    &[
        HardwareSupport {
            model: "B1",
            task: PrintTask::B1,
            status: SupportStatus::Tested,
            notes: "BLE print, QR+text, calibrate, info/RFID on real unit",
        },
        HardwareSupport {
            model: "B21 / B18",
            task: PrintTask::B21V1,
            status: SupportStatus::Experimental,
            notes: "Sequence from community wiki; not verified in this repo",
        },
        HardwareSupport {
            model: "D11 / D110",
            task: PrintTask::D110,
            status: SupportStatus::Experimental,
            notes: "Narrow head (96px); sequence simplified from wiki",
        },
        HardwareSupport {
            model: "other / unknown",
            task: PrintTask::Simple,
            status: SupportStatus::Experimental,
            notes: "1-byte PrintStart fallback; try if B1 task fails",
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportStatus {
    /// Run successfully on physical hardware by this project.
    Tested,
    /// Implemented from public protocol docs only.
    Experimental,
}

impl SupportStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tested => "tested",
            Self::Experimental => "experimental",
        }
    }
}

impl std::fmt::Display for SupportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HardwareSupport {
    pub model: &'static str,
    pub task: PrintTask,
    pub status: SupportStatus,
    pub notes: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b1_is_hardware_tested() {
        assert!(PrintTask::B1.hardware_tested());
        assert!(!PrintTask::B21V1.hardware_tested());
        assert!(!PrintTask::Simple.hardware_tested());
    }

    #[test]
    fn b1_print_start_is_seven_bytes() {
        let p = PrintTask::B1.print_start(1);
        assert_eq!(p.data.len(), 7);
        assert_eq!(&p.data[0..2], &1u16.to_be_bytes());
    }

    #[test]
    fn b1_page_size_is_six_bytes() {
        let p = PrintTask::B1.set_page_size(240, 384, 1);
        assert_eq!(p.data.len(), 6);
    }

    #[test]
    fn simple_page_size_is_four_bytes() {
        let p = PrintTask::Simple.set_page_size(100, 96, 1);
        assert_eq!(p.data.len(), 4);
    }

    #[test]
    fn for_model_mapping() {
        assert_eq!(PrintTask::for_model(Model::B1), PrintTask::B1);
        assert_eq!(PrintTask::for_model(Model::B21), PrintTask::B21V1);
        assert_eq!(PrintTask::for_model(Model::D11), PrintTask::D110);
    }

    #[test]
    fn effective_width_is_the_narrower_of_model_and_task() {
        use crate::geometry::{HEAD_NARROW_PX, HEAD_WIDE_PX};
        // Matched pairs keep their own width.
        assert_eq!(
            effective_max_width_px(Model::B1, PrintTask::B1),
            HEAD_WIDE_PX
        );
        assert_eq!(
            effective_max_width_px(Model::D110, PrintTask::D110),
            HEAD_NARROW_PX
        );
        // A mismatch takes the narrower limit from whichever side imposes it.
        assert_eq!(
            effective_max_width_px(Model::B1, PrintTask::D110),
            HEAD_NARROW_PX
        );
        assert_eq!(
            effective_max_width_px(Model::D110, PrintTask::B1),
            HEAD_NARROW_PX
        );
    }

    #[test]
    fn both_width_tables_agree_on_matched_pairs() {
        // The two tables are keyed differently; they must not drift apart for
        // the combinations that actually ship together.
        for model in [Model::B1, Model::B21, Model::B18, Model::D11, Model::D110] {
            let task = PrintTask::for_model(model);
            assert_eq!(
                model.max_width_px(),
                task.max_width_px(),
                "{model} and its default task {task} disagree on printhead width"
            );
        }
    }

    #[test]
    fn matrix_has_tested_b1() {
        let m = hardware_matrix();
        assert!(
            m.iter()
                .any(|h| h.model == "B1" && h.status == SupportStatus::Tested)
        );
    }
}
