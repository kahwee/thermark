//! Detected printer identity and model capabilities.

use crate::print_task::PrintTask;
use crate::protocol::Model;

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
    pub model_ids: &'static [u16],
    pub dpi: u16,
    pub max_width_px: u32,
    pub direction: PrintDirection,
    pub density_min: u8,
    pub density_max: u8,
    pub density_default: u8,
    pub label_types: &'static [u8],
    pub task: Option<PrintTask>,
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
    pub const fn print_path_hardware_tested(self, task: PrintTask) -> bool {
        matches!((self.model, task), (Model::B1, PrintTask::B1))
    }
}

const GAP_BLACK_TRANSPARENT: &[u8] = &[1, 2, 5];
const GAP_TRANSPARENT: &[u8] = &[1, 5];
const B18_MEDIA: &[u8] = &[1, 3, 5, 10, 11];

pub const PROFILES: &[PrinterProfile] = &[
    PrinterProfile {
        model: Model::B1,
        model_ids: &[4096],
        dpi: 203,
        max_width_px: 384,
        direction: PrintDirection::Top,
        density_min: 1,
        density_max: 5,
        density_default: 3,
        label_types: GAP_BLACK_TRANSPARENT,
        task: Some(PrintTask::B1),
    },
    PrinterProfile {
        model: Model::B1Pro,
        model_ids: &[4097],
        dpi: 300,
        max_width_px: 567,
        direction: PrintDirection::Top,
        density_min: 1,
        density_max: 5,
        density_default: 3,
        label_types: GAP_BLACK_TRANSPARENT,
        task: Some(PrintTask::D110MV4),
    },
    PrinterProfile {
        model: Model::B21Pro,
        model_ids: &[785],
        dpi: 300,
        max_width_px: 591,
        direction: PrintDirection::Top,
        density_min: 1,
        density_max: 5,
        density_default: 3,
        label_types: &[1, 2, 3, 5],
        task: Some(PrintTask::D110MV4),
    },
    PrinterProfile {
        model: Model::B18,
        model_ids: &[3584],
        dpi: 203,
        max_width_px: 96,
        direction: PrintDirection::Left,
        density_min: 1,
        density_max: 3,
        density_default: 2,
        label_types: B18_MEDIA,
        task: None,
    },
    PrinterProfile {
        model: Model::D11,
        model_ids: &[512],
        dpi: 203,
        max_width_px: 96,
        direction: PrintDirection::Left,
        density_min: 1,
        density_max: 3,
        density_default: 2,
        label_types: GAP_TRANSPARENT,
        task: Some(PrintTask::D11V1),
    },
    PrinterProfile {
        model: Model::D11H,
        model_ids: &[528],
        dpi: 300,
        max_width_px: 142,
        direction: PrintDirection::Left,
        density_min: 1,
        density_max: 5,
        density_default: 3,
        label_types: GAP_TRANSPARENT,
        task: Some(PrintTask::D110MV4),
    },
    PrinterProfile {
        model: Model::D110,
        model_ids: &[2304, 2305],
        dpi: 203,
        max_width_px: 96,
        direction: PrintDirection::Left,
        density_min: 1,
        density_max: 3,
        density_default: 2,
        label_types: GAP_TRANSPARENT,
        task: Some(PrintTask::D110),
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
        _ => profile.task,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b18_is_narrow_left_feed() {
        let profile = profile_for_model(Model::B18);
        assert_eq!(profile.max_width_px, 96);
        assert_eq!(profile.direction, PrintDirection::Left);
        assert_eq!((profile.density_min, profile.density_max), (1, 3));
        assert_eq!(profile.task, None);
    }

    #[test]
    fn hardware_verification_belongs_to_the_profile_and_task_pair() {
        assert!(profile_for_model(Model::B1).print_path_hardware_tested(PrintTask::B1));
        assert!(!profile_for_model(Model::B21Pro).print_path_hardware_tested(PrintTask::B1));
        assert!(!profile_for_model(Model::B1).print_path_hardware_tested(PrintTask::D110));
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
