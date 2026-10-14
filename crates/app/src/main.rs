//! lazypock CLI entry point.

mod cli;
mod shutdown;

use std::process::ExitCode;
use std::sync::Arc;

/// Whether to adopt tables lazypock did not create, from
/// `LAZYPOCK_REFLECT_EXISTING`. On by default: pointing lazypock at an existing
/// database is meant to work, and a table nobody can see is not working.
///
/// An unrecognised value is an error rather than a silent default, because the
/// wrong reading of "off" is a schema nobody can see.
fn reflect_existing() -> Result<bool> {
    match std::env::var("LAZYPOCK_REFLECT_EXISTING") {
        Err(_) => Ok(true),
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "on" | "true" | "yes" => Ok(true),
            "0" | "off" | "false" | "no" => Ok(false),
            other => Err(Error::config(format!(
                "LAZYPOCK_REFLECT_EXISTING must be on or off, got {other:?}"
            ))),
        },
    }
}

/// Where hook files live, from `LAZYPOCK_HOOKS_DIR`.
fn hooks_directory() -> String {
    std::env::var("LAZYPOCK_HOOKS_DIR").unwrap_or_else(|_| "hooks".to_string())
}
use std::time::Duration;

use clap::Parser;
use lazypock_api::router::router;
use lazypock_api::AppState;
use lazypock_core::{
    config::Config,
    error::{Error, Result},
    telemetry,
};
use lazypock_db::{migrate, pool::Db, schema::SchemaCache, watch::watch_schema};

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
            SuperuserCommand::Create { email, password } => {
                superuser_create(&config, &email, &password).await
            }
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

/// `lazypock superuser create --email ... --password ...`
async fn superuser_create(config: &Config, email: &str, password: &str) -> Result<()> {
    telemetry::init(config.log_level, config.log_format)?;

    let db = Db::from_config(config).await?;

    // Bring the database up to date first. Creating the very first superuser is
    // exactly the moment the schema may not exist yet, so this command cannot
    // assume migrations have run.
    {
        let mut client = db.client().await?;
        migrate::run(&mut client).await?;
    }

    let client = db.client().await?;
    let account = lazypock_auth::superuser::create(&client, email, password).await?;

    println!("created superuser {} ({})", account.email, account.id);
    Ok(())
}

async fn serve(config: Config) -> Result<()> {
    let addr = config.listen_addr;

    // Connect and load the schema up front: a node that cannot reach its
    // database, or whose system tables are missing, must fail at boot rather
    // than on the first request.
    let db = Db::from_config(&config).await?;
    let schema = Arc::new(SchemaCache::new());

    // Bring the database up to date before anything reads it. On a first run
    // against an empty database this is the whole installation step, and there is
    // no separate command to remember; against an existing database the runner
    // skips what is already applied.
    {
        let mut client = db.client().await?;
        let report = migrate::run(&mut client).await?;
        if !report.applied.is_empty() {
            tracing::info!(applied = ?report.applied, "migrations applied");
        }

        // A database that already holds somebody's tables should *work* when
        // lazypock is pointed at it, rather than showing an empty installation.
        // Adopting them is what makes that true: every table becomes a collection
        // and every column a field. Nothing here alters the existing tables.
        // A client's collection list should not be empty on a fresh installation, and
        // accounts need somewhere to live. Idempotent, so it is safe on every boot.
        lazypock_db::defaults::ensure_users(&mut client).await?;

        if reflect_existing()? {
            let reflected = lazypock_db::reflect::reflect(&client).await?;
            if !reflected.adopted.is_empty() {
                tracing::info!(
                    adopted = ?reflected.adopted,
                    fields = reflected.fields,
                    "adopted existing tables as collections"
                );
            }
            tracing::info!(
                skipped = reflected.skipped.len(),
                "tables left alone (internal, or already a collection)"
            );
        }
    }

    {
        let client = db.client().await?;
        schema
            .reload(&client)
            .await
            .map_err(|err| Error::config(format!("cannot load the schema: {err}")))?;
    }
    tracing::info!(collections = schema.snapshot().len(), "schema loaded");

    // Keep the cache fresh while the node runs: every schema mutation emits
    // NOTIFY lazypock_schema_changed at commit. The watcher holds its own
    // dedicated connection and reconnects on its own if it drops.
    let _watcher = watch_schema(schema.clone(), db.clone(), config.database_url.clone());

    // Load the token signing key before serving: a node whose key is missing
    // would reject every token, so it must fail at boot instead.
    let auth = Arc::new(lazypock_auth::AuthService::new(db.clone()));
    auth.load()
        .await
        .map_err(|err| Error::config(format!("cannot load the token signing key: {err}")))?;

    // Row-level security needs the restricted roles. Without them a request cannot
    // switch role, and the only safe reading is that it is not in force — so this
    // is loud rather than a debug line.
    {
        let mut client = db.client().await?;
        let unreachable = lazypock_db::rls::context::unreachable_roles(&mut client).await?;
        if !unreachable.is_empty() {
            tracing::warn!(
                unreachable = ?unreachable,
                "row-level security is NOT in force: this connection cannot switch to those roles. \
                 The connecting role must be a member of them, or be a superuser; see PLAN.md \
                 section 6."
            );
        }
    }

    // Realtime: every node listens on its own connection and fans out to its own
    // clients. A node that cannot listen still serves the API, but says so loudly.
    let hub = lazypock_realtime::Hub::new(db.clone(), schema.clone());
    match lazypock_realtime::PgBus::connect(&config.database_url).await {
        Ok(bus) => {
            tokio::spawn(hub.clone().run(bus));
            tracing::info!("realtime listening");
        }
        Err(err) => tracing::error!(
            error = %err,
            "realtime is unavailable: the LISTEN connection could not be established"
        ),
    }

    // File storage: configured from the environment, and it has to be usable at
    // boot rather than failing on the first upload.
    let files = Arc::new(
        lazypock_files::Files::from_config(&lazypock_files::FilesConfig::from_env()?)
            .map_err(|err| Error::config(format!("cannot configure file storage: {err}")))?,
    );
    files
        .storage()
        .health()
        .await
        .map_err(|err| Error::config(format!("file storage is not usable: {err}")))?;

    // The hook worker owns the QuickJS runtime on its own thread, because the
    // runtime is not `Send`. It starts even with no hook files: it costs an idle
    // thread, and it means a hook added later is picked up without a restart.
    let hooks = Arc::new(lazypock_hooks::HookService::start(
        hooks_directory(),
        lazypock_hooks::HookLimits::default(),
    ));

    let app = router(AppState::new(config, db, schema, auth, hub, files, hooks));

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
