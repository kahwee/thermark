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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli::run(cli).await {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(e) => {
            emit_error_tips(&e);
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    }
}
