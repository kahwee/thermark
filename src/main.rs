//! CLI entry point for pocket thermal label printers over BLE or USB serial.
//!
//! Everything beyond argument parsing and process exit lives in [`cli`].

mod cli;

use clap::Parser;
use cli::args::Cli;
use cli::tips::emit_error_tips;
use tracing_subscriber::EnvFilter;

fn init_tracing(verbose: bool) {
    let default = if verbose { "debug" } else { "info" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();
}

/// Exit code for "terminated by SIGINT", by shell convention (128 + 2).
const EXIT_INTERRUPTED: i32 = 130;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    // On Ctrl-C, drop the in-flight command rather than exiting from under it.
    // Dropping it drops the open session, and `BleTransport`'s `Drop` blocks
    // until the printer is disconnected — otherwise an interrupted print
    // leaves the single-client BLE link held until the printer times out.
    let code = tokio::select! {
        result = cli::run(cli) => match result {
            Ok(code) => code,
            Err(e) => {
                emit_error_tips(&e);
                eprintln!("error: {e:#}");
                1
            }
        },
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\ninterrupted — releasing the printer link");
            EXIT_INTERRUPTED
        }
    };

    if code != 0 {
        std::process::exit(code);
    }
}
