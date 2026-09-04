//! Detected printer identity and model capabilities.

use crate::config::ConnPref;
use crate::print_task::PrintTask;
use crate::protocol::Model;

/// Project verification status for a printer profile's default print path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportStatus {
    Tested,
    Experimental,
    Unresolved,
}

impl std::fmt::Display for SupportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::Tested => "tested",
            Self::Experimental => "experimental",
            Self::Unresolved => "unresolved",
        })
    }
}

/// Direction in which raster rows advance across the authored canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintDirection {
    Top,
    Left,
}

impl PrintDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Left => "left",
        }
    }
}

/// Facts reported by a connected printer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PrinterIdentity {
    pub model_id: u16,
    pub protocol_version: Option<u8>,
    pub firmware: Option<String>,
    pub hardware: Option<String>,
}

/// One coherent view of the connected printer.
///
/// Keeping the profile, selected protocol task, and observed identity together
/// prevents the client from accidentally updating one without the others.
#[derive(Debug, Clone)]
pub struct PrinterDevice {
    profile: &'static PrinterProfile,
    task: PrintTask,
    identity: Option<PrinterIdentity>,
}

impl PrinterDevice {
    pub fn configured(model: Model, task: PrintTask) -> Self {
        Self {
            profile: profile_for_model(model),
            task,
            identity: None,
        }
    }

    pub fn identify(
        &mut self,
        identity: PrinterIdentity,
        update_task: bool,
    ) -> Option<&'static PrinterProfile> {
        self.identity = Some(identity);
        let identity = self.identity.as_ref().expect("identity was just stored");
        let profile = profile_for_identity(identity)?;
        let detected_task = task_for_identity(identity);
        self.profile = profile;
        if update_task && let Some(task) = detected_task {
            self.task = task;
        }
        Some(profile)
    }

    pub const fn model(&self) -> Model {
        self.profile.model
    }

    pub const fn profile(&self) -> &'static PrinterProfile {
        self.profile
    }

    pub const fn task(&self) -> PrintTask {
        self.task
    }

    pub fn set_task(&mut self, task: PrintTask) {
        self.task = task;
    }

    pub fn identity(&self) -> Option<&PrinterIdentity> {
        self.identity.as_ref()
    }
}

/// Capabilities and protocol behavior for one printer model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrinterProfile {
    pub model: Model,
    /// Human-readable model name used in support summaries.
    pub display_name: &'static str,
    pub model_ids: &'static [u16],
    pub dpi: u16,
    pub max_width_px: u32,
    pub direction: PrintDirection,
    pub density_min: u8,
    pub density_max: u8,
    pub density_default: u8,
    pub label_types: &'static [u8],
    /// Default on-wire sequence for this model, if one is known.
    pub default_task: Option<PrintTask>,
    /// Verification status of this profile's default print path.
    pub support_status: SupportStatus,
    /// Connection on which the default path was physically verified.
    pub tested_connection: Option<ConnPref>,
    /// Concise evidence or limitation shown in support summaries.
    pub support_notes: &'static str,
}

impl PrinterProfile {
    pub const fn pixels_per_mm(self) -> f64 {
        self.dpi as f64 / 25.4
    }

    pub const fn supports_density(self, density: u8) -> bool {
        density >= self.density_min && density <= self.density_max
    }

    /// Whether this exact model/profile and print-task combination has been
    /// exercised on hardware owned by this project.
    ///
    /// A task describes wire behavior, not the physical device it is sent to.
    /// In particular, selecting the B1 task for another model does not make
    /// that printer path hardware-verified.
    pub const fn print_path_hardware_tested(self, task: PrintTask, connection: ConnPref) -> bool {
        matches!(
            self.print_path_support(task, connection),
            SupportStatus::Tested
        )
    }

    /// Support status for a selected task on this physical profile.
    ///
    /// Only the profile's registered default inherits its verification status;
    /// an override is experimental even when that wire sequence is tested on a
    /// different model.
    pub const fn print_path_support(self, task: PrintTask, connection: ConnPref) -> SupportStatus {
        if !same_task(self.default_task, task) {
            return SupportStatus::Experimental;
        }
        match self.support_status {
            SupportStatus::Tested if same_connection(self.tested_connection, Some(connection)) => {
                SupportStatus::Tested
            }
            SupportStatus::Tested => SupportStatus::Experimental,
            status => status,
        }
    }
}

const fn same_task(default: Option<PrintTask>, selected: PrintTask) -> bool {
    match default {
        Some(PrintTask::B1) => matches!(selected, PrintTask::B1),
        Some(PrintTask::D11V1) => matches!(selected, PrintTask::D11V1),
        Some(PrintTask::D110) => matches!(selected, PrintTask::D110),
        Some(PrintTask::D110MV4) => matches!(selected, PrintTask::D110MV4),
        None => false,
    }
}

const fn same_connection(left: Option<ConnPref>, right: Option<ConnPref>) -> bool {
    match left {
        Some(ConnPref::Ble) => matches!(right, Some(ConnPref::Ble)),
        Some(ConnPref::Usb) => matches!(right, Some(ConnPref::Usb)),
        None => right.is_none(),
    }
}

const GAP_BLACK_TRANSPARENT: &[u8] = &[1, 2, 5];
const GAP_TRANSPARENT: &[u8] = &[1, 5];
const B18_MEDIA: &[u8] = &[1, 3, 5, 10, 11];
const B1_SUPPORT_NOTES: &str = "BLE print, QR+text, calibrate, info/RFID on real unit";
const D110_MV4_SUPPORT_NOTES: &str = "complete 9-byte/13-byte community-validated sequence";

pub const PROFILES: &[PrinterProfile] = &[
    PrinterProfile {
        model: Model::B1,
        display_name: "B1",
        model_ids: &[4096],
        dpi: 203,
        max_width_px: 384,
        direction: PrintDirection::Top,
        density_min: 1,
        density_max: 5,
        density_default: 3,
        label_types: GAP_BLACK_TRANSPARENT,
        default_task: Some(PrintTask::B1),
        support_status: SupportStatus::Tested,
        tested_connection: Some(ConnPref::Ble),
        support_notes: B1_SUPPORT_NOTES,
    },
    PrinterProfile {
        model: Model::B1Pro,
        display_name: "B1 Pro",
        model_ids: &[4097],
        dpi: 300,
        max_width_px: 567,
        direction: PrintDirection::Top,
        density_min: 1,
        density_max: 5,
        density_default: 3,
        label_types: GAP_BLACK_TRANSPARENT,
        default_task: Some(PrintTask::D110MV4),
        support_status: SupportStatus::Experimental,
        tested_connection: None,
        support_notes: D110_MV4_SUPPORT_NOTES,
    },
    PrinterProfile {
        model: Model::B21Pro,
        display_name: "B21 Pro",
        model_ids: &[785],
        dpi: 300,
        max_width_px: 591,
        direction: PrintDirection::Top,
        density_min: 1,
        density_max: 5,
        density_default: 3,
        label_types: &[1, 2, 3, 5],
        default_task: Some(PrintTask::D110MV4),
        support_status: SupportStatus::Experimental,
        tested_connection: None,
        support_notes: D110_MV4_SUPPORT_NOTES,
    },
    PrinterProfile {
        model: Model::B18,
        display_name: "B18",
        model_ids: &[3584],
        dpi: 203,
        max_width_px: 96,
        direction: PrintDirection::Left,
        density_min: 1,
        density_max: 3,
        density_default: 2,
        label_types: B18_MEDIA,
        default_task: None,
        support_status: SupportStatus::Unresolved,
        tested_connection: None,
        support_notes: "96px left-feed geometry known; print sequence needs capture",
    },
    PrinterProfile {
        model: Model::D11,
        display_name: "D11",
        model_ids: &[512],
        dpi: 203,
        max_width_px: 96,
        direction: PrintDirection::Left,
        density_min: 1,
        density_max: 3,
        density_default: 2,
        label_types: GAP_TRANSPARENT,
        default_task: Some(PrintTask::D11V1),
        support_status: SupportStatus::Experimental,
        tested_connection: None,
        support_notes: "complete old-D11 sequence; firmware variants exist",
    },
    PrinterProfile {
        model: Model::D11H,
        display_name: "D11_H",
        model_ids: &[528],
        dpi: 300,
        max_width_px: 142,
        direction: PrintDirection::Left,
        density_min: 1,
        density_max: 5,
        density_default: 3,
        label_types: GAP_TRANSPARENT,
        default_task: Some(PrintTask::D110MV4),
        support_status: SupportStatus::Experimental,
        tested_connection: None,
        support_notes: D110_MV4_SUPPORT_NOTES,
    },
    PrinterProfile {
        model: Model::D110,
        display_name: "D110",
        model_ids: &[2304, 2305],
        dpi: 203,
        max_width_px: 96,
        direction: PrintDirection::Left,
        density_min: 1,
        density_max: 3,
        density_default: 2,
        label_types: GAP_TRANSPARENT,
        default_task: Some(PrintTask::D110),
        support_status: SupportStatus::Experimental,
        tested_connection: None,
        support_notes: "complete 1-byte/4-byte/quantity sequence",
    },
];

pub fn profile_for_model(model: Model) -> &'static PrinterProfile {
    PROFILES
        .iter()
        .find(|profile| profile.model == model)
        .expect("every Model must have a PrinterProfile")
}

pub fn profile_for_identity(identity: &PrinterIdentity) -> Option<&'static PrinterProfile> {
    PROFILES
        .iter()
        .find(|profile| profile.model_ids.contains(&identity.model_id))
}

/// Resolve firmware-sensitive task variants after the protocol handshake.
pub fn task_for_identity(identity: &PrinterIdentity) -> Option<PrintTask> {
    let profile = profile_for_identity(identity)?;
    match (profile.model, identity.protocol_version) {
        // Early D11 firmware uses the D110 flow; later variants use D11_V1.
        (Model::D11, Some(1 | 2)) => Some(PrintTask::D110),
        _ => profile.default_task,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::ValueEnum;

    #[test]
    fn b18_is_narrow_left_feed() {
        let profile = profile_for_model(Model::B18);
        assert_eq!(profile.max_width_px, 96);
        assert_eq!(profile.direction, PrintDirection::Left);
        assert_eq!((profile.density_min, profile.density_max), (1, 3));
        assert_eq!(profile.default_task, None);
    }

    #[test]
    fn hardware_verification_belongs_to_the_profile_task_and_connection() {
        assert!(
            profile_for_model(Model::B1).print_path_hardware_tested(PrintTask::B1, ConnPref::Ble)
        );
        assert!(
            !profile_for_model(Model::B1).print_path_hardware_tested(PrintTask::B1, ConnPref::Usb)
        );
        assert!(
            !profile_for_model(Model::B21Pro)
                .print_path_hardware_tested(PrintTask::B1, ConnPref::Ble)
        );
        assert!(
            !profile_for_model(Model::B1)
                .print_path_hardware_tested(PrintTask::D110, ConnPref::Ble)
        );
    }

    #[test]
    fn profiles_are_the_complete_support_registry() {
        let mut models = std::collections::HashSet::new();
        let mut model_ids = std::collections::HashSet::new();

        for profile in PROFILES {
            assert!(
                models.insert(profile.model),
                "duplicate {:?}",
                profile.model
            );
            assert!(!profile.display_name.is_empty());
            assert!(!profile.support_notes.is_empty());
            assert_eq!(
                profile.support_status == SupportStatus::Tested,
                profile.tested_connection.is_some(),
                "tested connection metadata for {:?}",
                profile.model
            );
            assert_eq!(
                PrintTask::for_model(profile.model),
                profile.default_task,
                "{:?}",
                profile.model
            );
            if let Some(default_task) = profile.default_task {
                let connection = profile.tested_connection.unwrap_or(ConnPref::Ble);
                assert_eq!(
                    profile.print_path_support(default_task, connection),
                    profile.support_status,
                    "default support path for {:?}",
                    profile.model
                );
            }
            for model_id in profile.model_ids {
                assert!(model_ids.insert(model_id), "duplicate model id {model_id}");
            }
        }

        let enum_models: std::collections::HashSet<_> =
            Model::value_variants().iter().copied().collect();
        assert_eq!(models, enum_models, "every Model variant needs one profile");
        let tested: Vec<_> = PROFILES
            .iter()
            .filter(|profile| profile.support_status == SupportStatus::Tested)
            .map(|profile| {
                (
                    profile.model,
                    profile.default_task,
                    profile.tested_connection,
                )
            })
            .collect();
        assert_eq!(
            tested,
            [(Model::B1, Some(PrintTask::B1), Some(ConnPref::Ble))]
        );
        assert_eq!(
            profile_for_model(Model::B18).support_status,
            SupportStatus::Unresolved
        );
    }

    #[test]
    fn identities_select_profiles() {
        let identity = PrinterIdentity {
            model_id: 4097,
            protocol_version: Some(5),
            firmware: None,
            hardware: None,
        };
        assert_eq!(profile_for_identity(&identity).unwrap().model, Model::B1Pro);
    }

    #[test]
    fn early_d11_firmware_selects_d110_task() {
        let identity = PrinterIdentity {
            model_id: 512,
            protocol_version: Some(1),
            firmware: None,
            hardware: None,
        };
        assert_eq!(task_for_identity(&identity), Some(PrintTask::D110));
        let later = PrinterIdentity {
            protocol_version: Some(3),
            ..identity
        };
        assert_eq!(task_for_identity(&later), Some(PrintTask::D11V1));
    }

    #[test]
    fn device_updates_profile_task_and_identity_together() {
        let mut device = PrinterDevice::configured(Model::B1, PrintTask::B1);
        let identity = PrinterIdentity {
            model_id: 4097,
            protocol_version: Some(5),
            firmware: Some("2.12".into()),
            hardware: Some("2.01".into()),
        };
        device.identify(identity.clone(), true).unwrap();
        assert_eq!(device.model(), Model::B1Pro);
        assert_eq!(device.task(), PrintTask::D110MV4);
        assert_eq!(device.identity(), Some(&identity));
    }

    #[test]
    fn device_keeps_unknown_identity_without_claiming_a_profile() {
        let mut device = PrinterDevice::configured(Model::B1, PrintTask::B1);
        let identity = PrinterIdentity {
            model_id: 0xffff,
            protocol_version: None,
            firmware: None,
            hardware: None,
        };
        assert_eq!(device.identify(identity.clone(), true), None);
        assert_eq!(device.identity(), Some(&identity));
        assert_eq!(device.model(), Model::B1);
    }
}
