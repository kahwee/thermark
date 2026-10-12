use super::*;
/// Reject an unverified configured task before opening a hardware connection.
/// The session checks again after identification because auto-detection may
/// replace the configured task with a different protocol sequence.
pub fn ensure_target_print_allowed(target: PrintTarget, connection: ConnPref) -> Result<()> {
    ensure_print_path_allowed(
        target.model,
        target.task.initial_task(),
        connection,
        target.allow_experimental,
    )
}

/// Require opt-in unless this exact physical profile and wire task form the
/// project's hardware-verified path.
pub(super) fn ensure_print_path_allowed(
    model: Model,
    task: PrintTask,
    connection: ConnPref,
    allow_experimental: bool,
) -> Result<()> {
    let profile = crate::profile_for_model(model);
    if !profile.print_path_hardware_tested(task, connection) && !allow_experimental {
        bail!(
            "printer profile '{model}' with print task '{task}' over '{connection}' is experimental \
             (not hardware-tested in this project). \
             Re-run with --allow-experimental if you accept the risk, \
             or use the hardware-tested B1 profile and task over BLE. See: thermark tasks"
        );
    }
    Ok(())
}
