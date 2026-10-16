//! Shared harness for the `lazypock-api` integration tests.
//!
//! Each test gets its own randomly named schema, so tests run in parallel and
//! cannot see each other's data. `TEST_DATABASE_URL` overrides the default; when
//! it is not set and Postgres is unreachable the tests skip with a message, but
//! when it *is* set (or `LAZYPOCK_REQUIRE_DB_TESTS=1`) an unreachable database
//! is a hard failure, so CI can never silently skip them.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{to_bytes, Body};
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use lazypock_core::config::Config;
use lazypock_db::pool::{Db, DbOptions};
use lazypock_db::schema::SchemaCache;
use lazypock_db::tokio_postgres::{self, NoTls};
use serde_json::Value;
use tower::ServiceExt;

/// Default connection: the development database.
pub const DEFAULT_TEST_URL: &str = "postgres://postgres:postgres@localhost:5432/lazypock_dev";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A running application bound to one disposable schema.
pub struct TestApp {
    /// The application state, ready to hand to `router`.
    pub state: lazypock_api::AppState,
    /// The same database handle, for arranging fixtures.
    pub db: Db,
    /// Connection string of the whole test database.
    admin_url: String,
    /// The schema this test is scoped to.
    schema: String,
    /// This test's hooks directory. Writing a file here and reloading is how a
    /// test installs a hook.
    pub hooks_directory: std::path::PathBuf,
}

impl TestApp {
    /// Check out a pooled connection.
    pub async fn client(&self) -> lazypock_db::deadpool_postgres::Client {
        self.db.client().await.expect("pooled client")
    }

    /// Re-read the collection schema into the cache.
    ///
    /// The cache is loaded once at boot and, in production, refreshed by the
    /// `lazypock_schema_changed` notification (Track F). Until that listener
    /// exists, a test that creates collections has to ask for the reload.
    pub async fn reload_schema(&self) {
        let client = self.db.client().await.expect("schema client");
        self.state
            .schema
            .reload(&client)
            .await
            .expect("reload schema");
    }

    /// Best-effort cleanup; a leftover schema in a disposable test database is
    /// harmless, so this never fails a test.
    pub async fn cleanup(&self) {
        if let Ok(admin) = admin_connect(&self.admin_url).await {
            let _ = admin
                .batch_execute(&format!(
                    "drop schema if exists \"{}\" cascade",
                    self.schema
                ))
                .await;
        }
    }
}

async fn admin_connect(url: &str) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
    let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

/// Prepare an isolated schema with the system migrations applied.
/// Initialise logging once, so an internal error the API redacts from the
/// response is still visible when a test runs with `--nocapture`.
fn init_logging() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = lazypock_core::telemetry::init(
            lazypock_core::config::LogLevel::Info,
            lazypock_core::config::LogFormat::Text,
        );
    });
}

/// An application bound to one disposable schema.
pub async fn test_app(label: &str) -> Option<TestApp> {
    test_app_with(label, &[]).await
}

/// Like [`test_app`], with extra configuration the test needs.
///
/// The knobs are the ones the server reads from the environment, so a test
/// configures a limit or a proxy list the way a deployment does.
pub async fn test_app_with(label: &str, extra: &[(&str, &str)]) -> Option<TestApp> {
    init_logging();
    // `LAZYPOCK_TEST_DATABASE_URL` first, then the app's own variable, so exporting
    // a single `LAZYPOCK_DATABASE_URL` is enough to point the tests somewhere.
    let explicit = std::env::var("LAZYPOCK_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("LAZYPOCK_DATABASE_URL"))
        .ok();
    let required = std::env::var("LAZYPOCK_REQUIRE_DB_TESTS").is_ok();
    let url = explicit
        .clone()
        .unwrap_or_else(|| DEFAULT_TEST_URL.to_string());

    let admin = match admin_connect(&url).await {
        Ok(admin) => admin,
        Err(err) => {
            if explicit.is_some() || required {
                panic!("Postgres is configured but unreachable: {err}");
            }
            eprintln!(
                "skipping {label}: Postgres unreachable ({err}). \
                 Set LAZYPOCK_DATABASE_URL to a reachable Postgres."
            );
            return None;
        }
    };

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let schema = format!(
        "t_{}_{}_{}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );

    admin
        .batch_execute(&format!("create schema \"{schema}\""))
        .await
        .expect("create test schema");

    let db = Db::connect(
        &url,
        &DbOptions {
            options: Some(format!("-c search_path={schema}")),
            ..Default::default()
        },
    )
    .await
    .expect("connect the pooled database handle");

    {
        let mut client = db.client().await.expect("migration client");
        lazypock_db::migrate::run(&mut client)
            .await
            .expect("apply system migrations");
    }

    let cache = Arc::new(SchemaCache::new());
    {
        let client = db.client().await.expect("schema client");
        cache.reload(&client).await.expect("load schema");
    }

    let config = Config::from_source(|key| match key {
        "LAZYPOCK_DATABASE_URL" => Some(url.clone()),
        // Whatever the test asked for, and nothing else: the ambient environment
        // must not change what a test measures.
        other => extra
            .iter()
            .find(|(name, _)| *name == other)
            .map(|(_, value)| value.to_string()),
    })
    .expect("test configuration");

    let auth = Arc::new(lazypock_auth::AuthService::new(db.clone()));
    auth.load().await.expect("load the token signing key");

    let hub = lazypock_realtime::Hub::new(db.clone(), cache.clone());
    // Each test gets its own storage directory, so uploads cannot leak between them.
    // Named after the schema, which is already unique (pid + clock + counter).
    // A clock value alone is not: two tests can read the same nanosecond, and
    // then they would share a store — or a hooks directory, which makes one
    // test's hook fire for another test's request.
    let files_directory = std::env::temp_dir().join(format!("lazypock-api-files-{schema}"));
    let files = Arc::new(
        lazypock_files::Files::from_config(&lazypock_files::FilesConfig {
            adapter: lazypock_files::StorageAdapter::Local,
            directory: files_directory.display().to_string(),
            public_base_url: None,
            endpoint: None,
            bucket: None,
            region: "auto".into(),
            access_key: None,
            secret_key: None,
            image_service_url: None,
            image_service_token: None,
            presign_ttl_seconds: lazypock_files::FilesConfig::DEFAULT_PRESIGN_TTL_SECONDS,
        })
        .expect("files"),
    );

    // Loaded from settings like the real node, so a test can set `cors_origins` and
    // see the enforcement.
    // Loaded from settings, as the real node does, so a test can set `cors_origins`
    // and see the enforcement.
    let cors = Arc::new(lazypock_api::cors::CorsPolicy::new());
    cors.load(&db.client().await.expect("cors client"))
        .await
        .expect("load the CORS allowlist");

    let hooks_directory = std::env::temp_dir().join(format!("lazypock-api-hooks-{schema}"));
    std::fs::create_dir_all(&hooks_directory).expect("hooks directory");
    let hooks = Arc::new(lazypock_hooks::HookService::start(
        &hooks_directory,
        lazypock_hooks::HookLimits::default(),
    ));

    Some(TestApp {
        state: lazypock_api::AppState::new(
            config,
            db.clone(),
            cache,
            lazypock_api::Services {
                auth,
                hub,
                files,
                hooks: hooks.clone(),
                mail_templates: Arc::new(
                    lazypock_mail::Templates::load(None).expect("mail templates"),
                ),
                cors: cors.clone(),
            },
        ),
        hooks_directory: hooks_directory.clone(),
        db,
        admin_url: url,
        schema,
    })
}

// --------------------------------------------------------------- HTTP helpers

/// One decoded response.
pub struct Reply {
    /// Status code.
    pub status: StatusCode,
    /// Response headers.
    pub headers: HeaderMap,
    /// Decoded JSON body; `None` when the response carried no body.
    pub body: Option<Value>,
}

/// Drive the router with a single request.
pub async fn call(app: &Router, method: Method, uri: &str, body: Option<Value>) -> Reply {
    let builder = Request::builder().method(method).uri(uri);

    let request = match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&value).expect("encode")))
            .expect("request"),
        None => builder.body(Body::empty()).expect("request"),
    };

    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        None
    } else {
        serde_json::from_slice(&bytes).ok()
    };

    Reply {
        status,
        headers,
        body,
    }
}

/// Drive the router with a bearer token.
pub async fn call_authed(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    token: &str,
) -> Reply {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));

    let request = match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&value).expect("encode")))
            .expect("request"),
        None => builder.body(Body::empty()).expect("request"),
    };

    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        None
    } else {
        serde_json::from_slice(&bytes).ok()
    };

    Reply {
        status,
        headers,
        body,
    }
}

/// Create a superuser and log in, returning an access token.
///
/// Shared so every test that needs a superuser token gets the same one, rather
/// than each file growing its own login dance.
pub async fn superuser_token(app: &TestApp, email: &str, password: &str) -> String {
    let client = app.client().await;
    // Ignore an existing account: a test may call this twice.
    let _ = lazypock_auth::superuser::create(&client, email, password).await;

    let reply = post(
        &lazypock_api::router::router(app.state.clone()),
        "/api/collections/_superusers/auth-with-password",
        serde_json::json!({ "identity": email, "password": password }),
    )
    .await;
    reply.body.expect("a login body")["token"]
        .as_str()
        .expect("a token")
        .to_string()
}

/// `GET` without a body.
pub async fn get(app: &Router, uri: &str) -> Reply {
    call(app, Method::GET, uri, None).await
}

/// `POST` with a JSON body.
pub async fn post(app: &Router, uri: &str, body: Value) -> Reply {
    call(app, Method::POST, uri, Some(body)).await
}
