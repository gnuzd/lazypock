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

/// The first superuser, from `LAZYPOCK_SUPERUSER_EMAIL` + `LAZYPOCK_SUPERUSER_PASSWORD`.
///
/// A deployment that cannot open a browser or `exec` into a container — Kubernetes, a
/// PaaS, a compose file a user is told to `up` — otherwise has no way to get its first
/// account, and the Studio's Setup page is only reachable if the server is exposed before
/// it has one. Both halves are supplied by the operator; **there is no default password**,
/// because a baked-in one is the most exploited thing in this class of product.
///
/// Three properties this deliberately has:
///
/// - **It never touches an existing installation.** If any superuser exists the variables
///   are ignored with a log line: a redeploy must not reset a password or create a second
///   account.
/// - **Both or neither.** Half a pair is a templating mistake, and guessing which half was
///   meant is how an account nobody can log into gets created.
/// - **It serialises with the Studio's Setup**, on the same advisory lock (`SETUP_LOCK`),
///   so a boot and a Setup request arriving together cannot both create one.
async fn bootstrap_superuser(db: &Db) -> Result<()> {
    let email = variable("LAZYPOCK_SUPERUSER_EMAIL");
    let password = variable("LAZYPOCK_SUPERUSER_PASSWORD");

    let (email, password) = match (email, password) {
        (None, None) => return Ok(()),
        (Some(_), None) => {
            return Err(Error::config(
                "LAZYPOCK_SUPERUSER_EMAIL is set but LAZYPOCK_SUPERUSER_PASSWORD is not; \
                 set both to create the first superuser, or neither",
            ))
        }
        (None, Some(_)) => {
            return Err(Error::config(
                "LAZYPOCK_SUPERUSER_PASSWORD is set but LAZYPOCK_SUPERUSER_EMAIL is not; \
                 set both to create the first superuser, or neither",
            ))
        }
        (Some(email), Some(password)) => (email, password),
    };

    let mut client = db.client().await?;
    let transaction = client.transaction().await.map_err(db_error)?;

    // The same lock the Setup endpoint takes, so the two cannot race.
    transaction
        .execute(
            "select pg_advisory_xact_lock($1::bigint)",
            &[&lazypock_auth::superuser::SETUP_LOCK],
        )
        .await
        .map_err(db_error)?;

    let existing = lazypock_auth::superuser::count(&transaction).await?;
    if existing > 0 {
        tracing::debug!(
            "LAZYPOCK_SUPERUSER_EMAIL is ignored: this installation already has a superuser"
        );
        return Ok(());
    }

    // `create` validates the address and refuses a password that is too short, so the
    // checks are the same ones the Setup page enforces.
    let account = lazypock_auth::superuser::create(&transaction, &email, &password).await?;
    transaction.commit().await.map_err(db_error)?;

    // The email, never the password.
    tracing::info!(
        email = %account.email,
        "created the first superuser from LAZYPOCK_SUPERUSER_EMAIL"
    );
    Ok(())
}

/// A non-empty environment variable, or `None`.
fn variable(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Whether a missing row-level-security role is fatal, from `LAZYPOCK_REQUIRE_RLS`.
///
/// On by default, because the alternative is not a degraded server: a request that cannot
/// switch role fails in `apply()`, so every anonymous and authenticated request answers
/// 500 while the Studio keeps working. A deployment that genuinely runs on superuser
/// tokens alone can say `LAZYPOCK_REQUIRE_RLS=off` and keep the warning.
///
/// An unrecognised value is an error rather than a silent default, for the same reason
/// `LAZYPOCK_REFLECT_EXISTING` treats one as an error: the wrong reading of "off" is a
/// server with no row-level security.
fn require_rls() -> Result<bool> {
    match std::env::var("LAZYPOCK_REQUIRE_RLS") {
        Err(_) => Ok(true),
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "on" | "true" | "yes" => Ok(true),
            "0" | "off" | "false" | "no" => Ok(false),
            other => Err(Error::config(format!(
                "LAZYPOCK_REQUIRE_RLS must be on or off, got {other:?}"
            ))),
        },
    }
}

/// Where mail template overrides live, from `LAZYPOCK_MAIL_TEMPLATES_DIR`.
///
/// Unset means the built-in templates only, which is what a deployment that has
/// not customised an email wants.
fn mail_templates_directory() -> Option<std::path::PathBuf> {
    std::env::var("LAZYPOCK_MAIL_TEMPLATES_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
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
use lazypock_db::{migrate, pool::db_error, pool::Db, schema::SchemaCache, watch::watch_schema};

use cli::{Cli, Command, SuperuserCommand};

/// Timeout for a single `healthcheck` probe.
const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// How many due cron jobs a single scheduling pass will fire.
///
/// A bound rather than "all of them", so a backlog cannot hold a pass open
/// indefinitely. The next tick picks up whatever this one left.
const CRON_BATCH: i64 = 16;

/// How many due jobs a single queue pass will run.
///
/// Bounded for the same reason as [`CRON_BATCH`], and so one slow job cannot hold
/// up every job behind it.
const JOB_BATCH: i64 = 16;

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

    // After the schema is up and before anything serves: a deployment with no browser and
    // no shell still needs a way to get its first account. A no-op unless both variables
    // are set, and a no-op on an installation that already has one.
    bootstrap_superuser(&db).await?;

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

    // Row-level security needs the restricted roles, and without them "running anyway" is
    // not a degraded mode: a request that cannot switch role fails in `apply()`, so every
    // anonymous and authenticated request answers 500 while superuser tokens keep working.
    // Failing here is the honest end of that: a node nobody can use is a worse outcome
    // than a node that says why it will not start. `LAZYPOCK_REQUIRE_RLS=off` is the
    // escape hatch for a deployment that really does run on superuser tokens alone.
    {
        let mut client = db.client().await?;
        let unreachable = lazypock_db::rls::context::unreachable_roles(&mut client).await?;
        if !unreachable.is_empty() {
            let explanation = format!(
                "this connection cannot switch to {unreachable:?}. The connecting role must be a \
                 member of them, or be a superuser; see PLAN.md section 6. Set \
                 LAZYPOCK_REQUIRE_RLS=off to serve without row-level security anyway (every \
                 anonymous and authenticated request will then fail)."
            );
            if require_rls()? {
                return Err(Error::config(format!(
                    "row-level security would not be in force: {explanation}"
                )));
            }
            tracing::warn!(
                unreachable = ?unreachable,
                "row-level security is NOT in force: {explanation}"
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
    // Storage: the environment's configuration, with anything a superuser saved over it, so
    // a change made in the Studio survives a restart as well as taking effect immediately.
    let mut files_config = lazypock_files::FilesConfig::from_env()?;
    {
        let client = db.client().await?;
        let setting = lazypock_files::storage_setting(&client).await?;
        if setting.as_object().is_some_and(|object| !object.is_empty()) {
            let secrets = lazypock_files::Secrets::from_env()?;
            files_config
                .apply_storage_setting(&setting, secrets.as_ref())
                .map_err(|err| {
                    Error::config(format!("cannot apply the saved storage settings: {err}"))
                })?;
            tracing::info!("applied the storage settings saved in the database");
        }
    }

    let files = Arc::new(
        lazypock_files::Files::from_config(&files_config)
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
            let reaper = lazypock_files::Reaper::new(files.storage(), files.backend_name())
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

    // The mail templates: the built-in defaults, with any file in the directory
    // overriding them, so an operator changes an email without a rebuild. A syntax
    // error is refused here rather than when the first email is sent.
    let mail_templates = Arc::new(
        lazypock_mail::Templates::load(mail_templates_directory().as_deref())
            .map_err(|err| Error::config(format!("cannot load the mail templates: {err}")))?,
    );

    let state = AppState::new(
        config,
        db,
        schema,
        lazypock_api::Services {
            auth,
            hub,
            files,
            hooks,
            mail_templates,
            cors,
        },
    );

    // The cron scheduler. `next_run_at` has been written since the admin API
    // landed, but nothing ever fired on it: a job only ran if an operator pressed
    // "run now", so a schedule looked live and did nothing. Before the state is
    // handed to the router, so a tick cannot race the first request.
    {
        let interval = cron_interval()?;
        if interval.is_zero() {
            tracing::warn!("LAZYPOCK_CRON_INTERVAL=0: scheduled jobs never fire");
        } else {
            tracing::info!(every_seconds = interval.as_secs(), "cron scheduler started");
            let state = state.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    ticker.tick().await;
                    match lazypock_api::crons::run_due(&state, CRON_BATCH).await {
                        Ok(0) => {}
                        Ok(ran) => tracing::info!(ran, "cron schedule fired"),
                        Err(err) => tracing::warn!(
                            error = %err.error,
                            "a cron scheduling pass failed"
                        ),
                    }
                }
            });
        }
    }

    // The job queue. `_jobs` and its claim index have existed since migration 1
    // and nothing wrote to them; this is the worker. The `mail` kind sends a
    // templated email, and the auth flows enqueue it; the "after" hooks are the
    // next caller.
    {
        let interval = job_interval()?;
        let lease = job_lease_seconds()?;
        if interval.is_zero() {
            tracing::warn!("LAZYPOCK_JOB_INTERVAL=0: queued jobs never run");
        } else {
            let worker = lazypock_core::id::new_id();
            tracing::info!(
                every_seconds = interval.as_secs(),
                lease_seconds = lease,
                "job queue worker started"
            );
            let state = state.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    ticker.tick().await;
                    match lazypock_api::jobs::run_due(&state, &worker, JOB_BATCH, lease).await {
                        Ok(0) => {}
                        Ok(ran) => tracing::info!(ran, "job queue ran"),
                        Err(err) => tracing::warn!(
                            error = %err.error,
                            "a job queue pass failed"
                        ),
                    }
                }
            });
        }
    }

    let app = router(state);

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

/// How often the cron scheduler looks for jobs that are due.
///
/// A pass with nothing due is one indexed query, so this can be frequent. The
/// granularity of a cron expression is a minute, so the default is half of that:
/// a job fires within thirty seconds of its time rather than up to a minute late.
/// `0` disables the scheduler — loudly, because a disabled scheduler means
/// `next_run_at` is written and never acted on, which is the bug this fixes.
fn cron_interval() -> Result<std::time::Duration> {
    let seconds = match std::env::var("LAZYPOCK_CRON_INTERVAL") {
        Err(_) => 30,
        Ok(raw) => raw.trim().parse::<u64>().map_err(|_| {
            Error::config(format!(
                "LAZYPOCK_CRON_INTERVAL must be a whole number of seconds, got {raw:?}"
            ))
        })?,
    };
    Ok(std::time::Duration::from_secs(seconds))
}

/// How often the job queue worker looks for work.
///
/// A pass with nothing to do is one indexed query, so this can be frequent.
/// Five seconds keeps a busy queue feeling immediate without polling hard when it
/// is empty. `0` disables the worker — loudly, because enqueued work would
/// otherwise sit there forever with nothing to say so.
fn job_interval() -> Result<std::time::Duration> {
    let seconds = match std::env::var("LAZYPOCK_JOB_INTERVAL") {
        Err(_) => 5,
        Ok(raw) => raw.trim().parse::<u64>().map_err(|_| {
            Error::config(format!(
                "LAZYPOCK_JOB_INTERVAL must be a whole number of seconds, got {raw:?}"
            ))
        })?,
    };
    Ok(std::time::Duration::from_secs(seconds))
}

/// How long a claim on a job is trusted before the job is handed back.
///
/// A worker that dies mid-job leaves the row `running`, and this is how long the
/// next pass waits before assuming that happened. Long enough that a slow job is
/// not taken away from a worker still running it.
fn job_lease_seconds() -> Result<f64> {
    let seconds = match std::env::var("LAZYPOCK_JOB_LEASE") {
        Err(_) => 300.0,
        Ok(raw) => raw.trim().parse::<f64>().map_err(|_| {
            Error::config(format!(
                "LAZYPOCK_JOB_LEASE must be a number of seconds, got {raw:?}"
            ))
        })?,
    };
    if seconds <= 0.0 {
        return Err(Error::config(
            "LAZYPOCK_JOB_LEASE must be greater than zero".to_string(),
        ));
    }
    Ok(seconds)
}
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
