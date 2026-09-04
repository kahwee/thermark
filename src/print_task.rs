//! Complete on-wire print sequences used by supported protocol generations.

use crate::packet::Packet;
use crate::protocol::Model;
use crate::protocol::{self, Cmd};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum PrintTask {
    #[default]
    #[value(name = "b1")]
    B1,
    #[value(name = "d11v1")]
    D11V1,
    #[value(name = "d110")]
    D110,
    #[value(name = "d110mv4")]
    D110MV4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    Status,
    PageIndex,
}

impl PrintTask {
    pub fn for_model(model: Model) -> Option<Self> {
        crate::profile::profile_for_model(model).default_task
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "b1" => Some(Self::B1),
            "d11v1" => Some(Self::D11V1),
            "d110" => Some(Self::D110),
            "d110mv4" => Some(Self::D110MV4),
            _ => None,
        }
    }

    pub fn print_start(self, total_pages: u16) -> Packet {
        let [hi, lo] = total_pages.to_be_bytes();
        match self {
            Self::B1 => protocol::pkt(Cmd::PrintStart, vec![hi, lo, 0, 0, 0, 0, 0]),
            Self::D11V1 | Self::D110 => protocol::pkt(Cmd::PrintStart, vec![1]),
            Self::D110MV4 => protocol::pkt(Cmd::PrintStart, vec![hi, lo, 0, 0, 0, 0, 0, 1, 0]),
        }
    }

    pub fn set_page_size(self, rows: u16, cols: u16, copies: u16) -> Packet {
        let mut data = Vec::new();
        data.extend_from_slice(&rows.to_be_bytes());
        match self {
            Self::D11V1 => {}
            Self::D110 => data.extend_from_slice(&cols.to_be_bytes()),
            Self::B1 => {
                data.extend_from_slice(&cols.to_be_bytes());
                data.extend_from_slice(&copies.to_be_bytes());
            }
            Self::D110MV4 => {
                data.extend_from_slice(&cols.to_be_bytes());
                data.extend_from_slice(&copies.to_be_bytes());
                data.extend_from_slice(&[0; 7]);
            }
        }
        protocol::pkt(Cmd::SetPageSize, data)
    }

    pub const fn uses_print_clear(self) -> bool {
        matches!(self, Self::D11V1 | Self::D110)
    }

    pub const fn uses_page_start(self) -> bool {
        !matches!(self, Self::D110MV4)
    }

    pub const fn uses_print_quantity(self) -> bool {
        matches!(self, Self::D11V1 | Self::D110)
    }

    pub const fn pre_page_status(self) -> bool {
        matches!(self, Self::D110MV4)
    }

    pub const fn heartbeat_after_end(self) -> bool {
        matches!(self, Self::D110MV4)
    }

    pub const fn completion(self) -> Completion {
        match self {
            Self::D11V1 => Completion::PageIndex,
            _ => Completion::Status,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::B1 => "b1",
            Self::D11V1 => "d11v1",
            Self::D110 => "d110",
            Self::D110MV4 => "d110mv4",
        }
    }
}

impl std::fmt::Display for PrintTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

impl std::str::FromStr for PrintTask {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
            .ok_or_else(|| format!("unknown print task '{s}' (try b1, d11v1, d110, d110mv4)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_payload_shapes() {
        assert_eq!(PrintTask::B1.print_start(1).data.len(), 7);
        assert_eq!(PrintTask::D110MV4.print_start(1).data.len(), 9);
        assert_eq!(PrintTask::D11V1.set_page_size(100, 96, 1).data.len(), 2);
        assert_eq!(PrintTask::D110.set_page_size(100, 96, 1).data.len(), 4);
        assert_eq!(PrintTask::B1.set_page_size(100, 96, 1).data.len(), 6);
        assert_eq!(PrintTask::D110MV4.set_page_size(100, 96, 1).data.len(), 13);
    }

    #[test]
    fn legacy_task_names_are_rejected() {
        for name in ["simple", "niimprint", "b21", "b21v1", "d11", "v4"] {
            assert_eq!(PrintTask::parse(name), None, "{name}");
        }
    }
}
