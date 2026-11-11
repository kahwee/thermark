//! End-to-end security boundaries that do not touch printer hardware.
use super::*;

#[cfg(unix)]
#[test]
fn saved_label_is_private_and_refuses_destination_symlink() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = CliFixture::new();
    let path = fixture.dir.path().join("label.png");
    let target = fixture.dir.path().join("other-file");
    std::fs::write(&target, b"untouched").unwrap();
    symlink(&target, &path).unwrap();
    fixture
        .command()
        .args(["text", "--text", "Hello", "--no-print", "--save"])
        .arg(&path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("symlink"));
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
    std::fs::remove_file(&path).unwrap();
    fixture
        .command()
        .args(["text", "--text", "Hello", "--no-print", "--save"])
        .arg(&path)
        .assert()
        .success();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn legacy_saved_ble_name_fails_before_contacting_hardware() {
    let fixture = CliFixture::new();
    std::fs::write(fixture.config(), r#"{"addr":"B1-Old","connection":"ble"}"#).unwrap();
    fixture
        .command()
        .arg("info")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not pinned to a platform ID"));
    fixture
        .command()
        .args(["doctor", "--use-config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not pinned to a platform ID"));
}

#[test]
fn malicious_filename_cannot_emit_terminal_commands_in_error() {
    let fixture = CliFixture::new();
    let bad = fixture.dir.path().join("\x1b]52;c;PAYLOAD\x07\u{202e}.ttf");
    let output = fixture
        .command()
        .args(["text", "--text", "Hello", "--no-print", "--font"])
        .arg(bad)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains('\x1b'));
    assert!(!stderr.contains('\x07'));
    assert!(!stderr.contains('\u{202e}'));
}

#[test]
fn oversized_config_is_rejected_before_json_parsing() {
    let fixture = CliFixture::new();
    let file = std::fs::File::create(fixture.config()).unwrap();
    file.set_len(1024 * 1024 + 1).unwrap();
    fixture
        .command()
        .args(["config", "show"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("byte limit"));
}

#[cfg(unix)]
#[test]
fn fifo_inputs_fail_without_waiting_for_a_writer() {
    use std::time::Duration;
    let fixture = CliFixture::new();
    let fifo = fixture.dir.path().join("input.pipe");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    fixture
        .command_with_config(&fifo)
        .args(["config", "show"])
        .timeout(Duration::from_secs(5))
        .assert()
        .failure()
        .stderr(predicate::str::contains("regular file"));
    fixture
        .command()
        .args(["batch", "--csv"])
        .arg(&fifo)
        .arg("--preview-dir")
        .arg(fixture.dir.path().join("output"))
        .timeout(Duration::from_secs(5))
        .assert()
        .failure()
        .stderr(predicate::str::contains("regular file"));
    let job = fixture.dir.path().join("job");
    std::fs::create_dir(&job).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(job.join("manifest.json"))
            .status()
            .unwrap()
            .success()
    );
    fixture
        .command()
        .args(["job", "verify", "--job"])
        .arg(job)
        .timeout(Duration::from_secs(5))
        .assert()
        .failure()
        .stderr(predicate::str::contains("regular file"));
}
