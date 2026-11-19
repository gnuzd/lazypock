mod cli;
mod commands;
mod shutdown;

use clap::Parser;
use std::process::ExitCode;

use cli::{Cli, Command};
use lazypock_core::{Config, Result, telemetry};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("lazypock: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::form_env()?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => {
            telemetry::init(config.log_level, config.log_format)?;

            tracing::info!(
                database = %config.redacted_database_url(),
                "lazypock starting"
            );

            commands::serve::run(config).await
        }
        Command::Healthcheck => commands::health::healthcheck(&config).await,
    }
}
