//! CLI behavior tests; every child command starts with isolated configuration.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

/// Own the default configuration directory until all child commands finish.
struct CliFixture {
    dir: tempfile::TempDir,
}

impl CliFixture {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("CLI workspace"),
        }
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("config.json")
    }

    fn command(&self) -> Command {
        self.command_with_config(&self.config())
    }

    /// Explicit overrides still remove inherited printer addresses.
    fn command_with_config(&self, path: &Path) -> Command {
        let mut command = Command::cargo_bin("thermark").expect("binary thermark");
        command
            .env("THERMARK_CONFIG", path)
            .env_remove("THERMARK_ADDR");
        command
    }
}

#[path = "cli/commands.rs"]
mod commands;
#[path = "cli/config.rs"]
mod config;
#[path = "cli/rendering.rs"]
mod rendering;
