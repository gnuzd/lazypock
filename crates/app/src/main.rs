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

    // Realtime: every node listens on its own connection and fans out to its own
    // clients. A node that cannot listen still serves the API, but says so loudly.
    // Created before the schema watcher so the watcher can announce a registry
    // change to this node's clients.
    let hub = lazypock_realtime::Hub::new(db.clone(), schema.clone());

    // Keep the cache fresh while the node runs: every schema mutation emits
    // NOTIFY lazypock_schema_changed at commit. The watcher holds its own
    // dedicated connection and reconnects on its own if it drops. After each
    // reload it also tells the realtime registry subscribers, so a client's
    // collection list refreshes after a change made on any node.
    let registry_notifier = {
        let hub = hub.clone();
        Arc::new(move || hub.notify_collections_changed())
    };
    let _watcher = watch_schema(
        schema.clone(),
        db.clone(),
        config.database_url.clone(),
        Some(registry_notifier),
    );

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

    // Realtime is listening on this node now (see the watcher above).
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

    // The file reaper. The `_files_deleted` trigger queues an object's removal in the
    // transaction that deleted its row; this is what actually removes it, plus any
    // editor or library upload nobody ever attached. Before this there was no worker at
    // all, so the queue only drained if an operator called `POST /api/files/sweep` —
    // "deleting a record removes its files" was true of the rows and not of the objects.
    {
        let interval = file_reaper_interval()?;
        if interval.is_zero() {
            tracing::warn!("LAZYPOCK_FILE_REAPER_INTERVAL=0: deleted objects stay in storage");
        } else {
            let ttl = unattached_upload_ttl(&db).await?;
            let reaper = lazypock_files::Reaper::new(files.storage().clone(), files.backend_name())
                .with_unattached_ttl(ttl);
            tracing::info!(
                every_seconds = interval.as_secs(),
                unattached_ttl_seconds = ttl,
                "file reaper started"
            );

            let db = db.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    ticker.tick().await;
                    match db.client().await {
                        Ok(client) => match reaper.run(&client).await {
                            Ok(report) if !report.is_empty() => tracing::info!(
                                deleted = report.deleted,
                                failed = report.failed,
                                reclaimed = report.reclaimed,
                                "file reaper finished a pass"
                            ),
                            Ok(_) => {}
                            Err(err) => {
                                tracing::warn!(error = %err, "a file reaper pass failed")
                            }
                        },
                        Err(err) => tracing::warn!(
                            error = %err,
                            "the file reaper could not reach the database"
                        ),
                    }
                }
            });
        }
    }

    // The hook worker owns the QuickJS runtime on its own thread, because the
    // runtime is not `Send`. It starts even with no hook files: it costs an idle
    // thread, and it means a hook added later is picked up without a restart.
    let hooks = Arc::new(lazypock_hooks::HookService::start(
        hooks_directory(),
        lazypock_hooks::HookLimits::default(),
    ));

    // CORS is enforced from the `cors_origins` setting, so it is a runtime value
    // rather than a startup one; `POST /api/settings/refresh-cors` reloads it.
    let cors = Arc::new(lazypock_api::cors::CorsPolicy::new());
    {
        let client = db.client().await?;
        cors.load(&client)
            .await
            .map_err(|err| Error::config(format!("cannot load the CORS allowlist: {err}")))?;
    }

    let app = router(AppState::new(
        config,
        db,
        schema,
        lazypock_api::Services {
            auth,
            hub,
            files,
            hooks,
            cors,
        },
    ));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| Error::internal(format!("failed to bind {addr}: {err}")))?;

    tracing::info!(%addr, "listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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

/// How often the file reaper runs.
///
/// A pass with nothing to do is three indexed queries, so the default is a compromise
/// between promptness and noise: an hour. `0` disables the worker — loudly, because a
/// disabled reaper means deleted objects sit in storage forever.
fn file_reaper_interval() -> Result<std::time::Duration> {
    let seconds = match std::env::var("LAZYPOCK_FILE_REAPER_INTERVAL") {
        Err(_) => 3600,
        Ok(raw) => raw.trim().parse::<u64>().map_err(|_| {
            Error::config(format!(
                "LAZYPOCK_FILE_REAPER_INTERVAL must be a whole number of seconds, got {raw:?}"
            ))
        })?,
    };
    Ok(std::time::Duration::from_secs(seconds))
}

/// How long an unattached editor or library upload is kept.
///
/// Read from the `files` setting the Studio's Media page writes
/// (`files.rules.unattached_ttl_ms`), falling back to a day. An upload that arrived
/// through a record's field is never reclaimed, whatever this says.
async fn unattached_upload_ttl(db: &Db) -> Result<i64> {
    const DEFAULT_SECONDS: i64 = 24 * 60 * 60;

    let client = db.client().await?;
    // The settings row is one jsonb object; `->>` pulls the number out as text, so this
    // needs no JSON type here.
    let row = client
        .query_opt(
            "select value -> 'rules' ->> 'unattached_ttl_ms' from _settings where key = 'files'",
            &[],
        )
        .await
        .map_err(lazypock_db::pool::db_error)?;

    let Some(row) = row else {
        return Ok(DEFAULT_SECONDS);
    };
    let milliseconds: Option<i64> = row
        .get::<_, Option<String>>(0)
        .and_then(|raw| raw.parse().ok());
    Ok(milliseconds.map(|ms| ms / 1000).unwrap_or(DEFAULT_SECONDS))
}
