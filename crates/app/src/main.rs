//! lazypock CLI entry point.

mod cli;
mod shutdown;

use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use lazypock_api::router::{router, AppState};
use lazypock_core::{
    config::Config,
    error::{Error, Result},
    telemetry,
};

use lazypock_db::{migrate, pool::Db};

use cli::{Cli, Command, SuperuserCommand};

/// Timeout for a single `healthcheck` probe.
const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> ExitCode {
    // Best-effort .env loading; real deployments inject the environment.
    let _ = dotenvy::dotenv();

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
    let config = Config::from_env()?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => {
            telemetry::init(config.log_level, config.log_format)?;
            tracing::info!(
                env = config.env.as_str(),
                database = %config.redacted_database_url(),
                "lazypock starting"
            );
            serve(config).await
        }
        Command::Config => {
            print!("{config}");
            Ok(())
        }
        Command::Healthcheck => healthcheck(&config).await,
        Command::Migrate { status } => migrate_command(&config, status).await,
        Command::Superuser { command } => match command {
            SuperuserCommand::Create { .. } => Err(Error::unsupported(
                "`superuser create` is not implemented yet (PLAN.md Phase 2, Track E)",
            )),
        },
    }
}

/// `lazypock migrate [--status]`.
async fn migrate_command(config: &Config, status_only: bool) -> Result<()> {
    telemetry::init(config.log_level, config.log_format)?;

    let db = Db::from_config(config).await?;
    let mut client = db.client().await?;

    if status_only {
        for status in migrate::status(&client).await? {
            let state = match (status.applied, status.checksum_matches) {
                (false, _) => "pending",
                (true, true) => "applied",
                (true, false) => "applied (CHECKSUM CHANGED)",
            };
            println!("{:>6}  {:<24} {state}", status.version, status.name);
        }
        return Ok(());
    }

    let report = migrate::run(&mut client).await?;
    if report.applied.is_empty() {
        println!(
            "no pending migrations ({} already applied)",
            report.already_applied.len()
        );
    } else {
        println!(
            "applied {} migration(s): {:?}",
            report.applied.len(),
            report.applied
        );
    }
    Ok(())
}

async fn serve(config: Config) -> Result<()> {
    let addr = config.listen_addr;
    let app = router(AppState::new(config));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| Error::internal(format!("failed to bind {addr}: {err}")))?;

    tracing::info!(%addr, "listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown::signal())
        .await
        .map_err(|err| Error::internal(format!("server error: {err}")))?;

    Ok(())
}

/// Probe our own `/api/health` endpoint over a plain TCP connection.
///
/// Implemented without an HTTP client dependency: the probe is one request
/// against a known path, and keeping the binary small is a project priority.
async fn healthcheck(config: &Config) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = config.client_addr();

    let probe = async {
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|err| Error::internal(format!("cannot connect to {addr}: {err}")))?;

        let request =
            format!("GET /api/health HTTP/1.0\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|err| Error::internal(format!("failed to send probe: {err}")))?;
        stream
            .flush()
            .await
            .map_err(|err| Error::internal(format!("failed to flush probe: {err}")))?;

        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .map_err(|err| Error::internal(format!("failed to read probe response: {err}")))?;

        Ok::<String, Error>(String::from_utf8_lossy(&response).into_owned())
    };

    let response = tokio::time::timeout(HEALTHCHECK_TIMEOUT, probe)
        .await
        .map_err(|_| Error::internal(format!("healthcheck against {addr} timed out")))??;

    let status_line = response.lines().next().unwrap_or_default();
    if status_line.contains(" 200") {
        println!("ok");
        Ok(())
    } else {
        Err(Error::internal(format!(
            "unhealthy response from {addr}: {status_line}"
        )))
    }
}
