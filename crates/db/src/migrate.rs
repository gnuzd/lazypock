//! Versioned migration runner.
//!
//! * System migrations are compiled into the binary, so they apply even when
//!   the `migrations/` directory is absent.
//! * Each run takes a Postgres **advisory lock**, so when several nodes boot at
//!   once only one of them migrates and the others wait (see `PLAN.md` §9).
//! * Every migration is applied inside its own transaction together with its
//!   ledger row: DDL in Postgres is transactional, so a failure leaves no
//!   half-applied migration.
//! * The ledger records a checksum per migration. An edited or unknown applied
//!   migration is a hard error rather than a silent divergence.

use lazypock_core::error::{Error, Result};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;

use crate::pool::db_error;

/// Advisory-lock key that serialises migration runs across nodes.
///
/// The ASCII bytes of "lazypock"; any constant works as long as it is stable.
const MIGRATION_LOCK_KEY: i64 = 0x6c61_7a79_706f_636b;

/// Ledger DDL.
///
/// Must exist before the first migration can be recorded, so it is created up
/// front. Mirrors the definition in `0001_system_tables.sql` and is idempotent.
const LEDGER_DDL: &str = "\
create table if not exists _migrations (
    version    bigint primary key,
    name       text        not null,
    checksum   text        not null,
    applied_at timestamptz not null default now()
)";

/// A single migration compiled into the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// Monotonically increasing version. Applied in ascending order.
    pub version: i64,
    /// Stable human-readable name.
    pub name: &'static str,
    /// The SQL to execute.
    pub sql: &'static str,
}

/// Every known system migration, in application order.
pub static SYSTEM_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "system_tables",
        sql: include_str!("../../../migrations/0001_system_tables.sql"),
    },
    Migration {
        version: 2,
        name: "superusers",
        sql: include_str!("../../../migrations/0002_superusers.sql"),
    },
    Migration {
        version: 3,
        name: "rls",
        sql: include_str!("../../../migrations/0003_rls.sql"),
    },
    Migration {
        version: 4,
        name: "realtime",
        sql: include_str!("../../../migrations/0004_realtime.sql"),
    },
    Migration {
        version: 5,
        name: "image_presets",
        sql: include_str!("../../../migrations/0005_image_presets.sql"),
    },
    Migration {
        version: 6,
        name: "auth_system_fields",
        sql: include_str!("../../../migrations/0006_auth_system_fields.sql"),
    },
    Migration {
        version: 7,
        name: "request_logs",
        sql: include_str!("../../../migrations/0007_request_logs.sql"),
    },
    Migration {
        version: 8,
        name: "crons",
        sql: include_str!("../../../migrations/0008_crons.sql"),
    },
    Migration {
        version: 9,
        name: "import_snapshots",
        sql: include_str!("../../../migrations/0009_import_snapshots.sql"),
    },
    Migration {
        version: 10,
        name: "token_key_is_not_a_field",
        sql: include_str!("../../../migrations/0010_token_key_is_not_a_field.sql"),
    },
    Migration {
        version: 11,
        name: "files",
        sql: include_str!("../../../migrations/0011_files.sql"),
    },
    Migration {
        version: 12,
        name: "oauth2",
        sql: include_str!("../../../migrations/0012_oauth2.sql"),
    },
    Migration {
        version: 13,
        name: "otps",
        sql: include_str!("../../../migrations/0013_otps.sql"),
    },
    Migration {
        version: 14,
        name: "api_keys",
        sql: include_str!("../../../migrations/0014_api_keys.sql"),
    },
];

/// Outcome of a migration run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Versions applied by this run.
    pub applied: Vec<i64>,
    /// Versions that were already applied and unchanged.
    pub already_applied: Vec<i64>,
}

/// State of a known migration relative to the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStatus {
    /// Migration version.
    pub version: i64,
    /// Migration name.
    pub name: String,
    /// Whether it has been applied.
    pub applied: bool,
    /// Whether the recorded checksum matches the compiled-in SQL.
    pub checksum_matches: bool,
}

/// SHA-256 checksum of a migration's SQL, hex encoded.
pub fn checksum(sql: &str) -> String {
    let digest = Sha256::digest(sql.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Apply every pending system migration.
///
/// Safe to call concurrently from multiple processes: the advisory lock
/// serialises them, and the ledger makes double-application a no-op.
pub async fn run(client: &mut Client) -> Result<MigrationReport> {
    // Take the lock *before* touching the ledger: `create table if not exists`
    // from two connections at once races on the system catalogs even though it
    // looks idempotent, which is exactly what happens when several nodes boot
    // together.
    lock(client).await?;

    let outcome = run_locked(client).await;

    // Always release before returning: session-level advisory locks survive
    // connection reuse in the pool, so leaking one would deadlock every later
    // migration run.
    let unlocked = unlock(client).await;

    let report = outcome?;
    unlocked?;
    Ok(report)
}

/// Take the migration advisory lock on `client`.
async fn lock(client: &Client) -> Result<()> {
    client
        .execute(
            "select pg_advisory_lock($1::bigint)",
            &[&MIGRATION_LOCK_KEY],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Release the migration advisory lock on `client`.
async fn unlock(client: &Client) -> Result<()> {
    client
        .execute(
            "select pg_advisory_unlock($1::bigint)",
            &[&MIGRATION_LOCK_KEY],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Report which known migrations are applied and whether the database contains
/// migrations this binary does not know about.
pub async fn status(client: &Client) -> Result<Vec<MigrationStatus>> {
    // Serialise with migration runs so the ledger cannot be created
    // concurrently and the result is consistent with a run in flight.
    lock(client).await?;
    let outcome = status_locked(client).await;
    let unlocked = unlock(client).await;
    let statuses = outcome?;
    unlocked?;
    Ok(statuses)
}

async fn status_locked(client: &Client) -> Result<Vec<MigrationStatus>> {
    ensure_ledger(client).await?;
    let applied = load_applied(client).await?;

    Ok(SYSTEM_MIGRATIONS
        .iter()
        .map(|migration| {
            let recorded = applied
                .iter()
                .find(|(version, _, _)| *version == migration.version);
            MigrationStatus {
                version: migration.version,
                name: migration.name.to_string(),
                applied: recorded.is_some(),
                checksum_matches: recorded
                    .map(|(_, _, sum)| sum == &checksum(migration.sql))
                    .unwrap_or(false),
            }
        })
        .collect())
}

async fn ensure_ledger(client: &Client) -> Result<()> {
    // Check first instead of relying on `create table if not exists`: the latter
    // emits a NOTICE on every run, which is pure log noise on a busy node.
    // Callers hold the migration lock, so this cannot race.
    let exists: bool = client
        .query_one("select to_regclass('_migrations') is not null", &[])
        .await
        .map_err(db_error)?
        .get(0);
    if exists {
        return Ok(());
    }

    client
        .batch_execute(LEDGER_DDL)
        .await
        .map_err(|err| Error::internal(format!("failed to create the migration ledger: {err}")))
}

/// Returns `(version, name, checksum)` ordered by version.
async fn load_applied(client: &Client) -> Result<Vec<(i64, String, String)>> {
    let rows = client
        .query(
            "select version, name, checksum from _migrations order by version",
            &[],
        )
        .await
        .map_err(db_error)?;
    Ok(rows
        .iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect())
}

async fn run_locked(client: &mut Client) -> Result<MigrationReport> {
    ensure_ledger(client).await?;
    let applied = load_applied(client).await?;

    // A migration recorded in the database that this binary does not know about
    // means the database is ahead of the code: refuse to guess.
    for (version, name, _) in &applied {
        if !SYSTEM_MIGRATIONS.iter().any(|m| m.version == *version) {
            return Err(Error::invalid(format!(
                "database has migration {version} ({name}) applied but this binary does not know it; \
                 the database is newer than the server"
            )));
        }
    }

    let mut report = MigrationReport::default();

    for migration in SYSTEM_MIGRATIONS {
        match applied
            .iter()
            .find(|(version, _, _)| *version == migration.version)
        {
            Some((_, name, recorded_checksum)) => {
                let current = checksum(migration.sql);
                if recorded_checksum != &current {
                    return Err(Error::invalid(format!(
                        "migration {} ({name}) was modified after it was applied \
                         (recorded checksum {recorded_checksum}, current {current}); \
                         add a new migration instead of editing an applied one",
                        migration.version
                    )));
                }
                report.already_applied.push(migration.version);
            }
            None => {
                apply(client, migration).await?;
                report.applied.push(migration.version);
                tracing::info!(
                    version = migration.version,
                    name = migration.name,
                    "applied migration"
                );
            }
        }
    }

    Ok(report)
}

async fn apply(client: &mut Client, migration: &Migration) -> Result<()> {
    let transaction = client.transaction().await.map_err(db_error)?;

    transaction
        .batch_execute(migration.sql)
        .await
        .map_err(|err| {
            Error::internal(format!(
                "migration {} ({}) failed: {err}",
                migration.version, migration.name
            ))
        })?;

    transaction
        .execute(
            "insert into _migrations (version, name, checksum) values ($1, $2, $3)",
            &[
                &migration.version,
                &migration.name,
                &checksum(migration.sql),
            ],
        )
        .await
        .map_err(db_error)?;

    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_unique_and_ordered() {
        let mut versions: Vec<i64> = SYSTEM_MIGRATIONS.iter().map(|m| m.version).collect();
        let count = versions.len();
        versions.sort_unstable();
        versions.dedup();
        assert_eq!(versions.len(), count, "duplicate migration version");
        assert!(
            versions.windows(2).all(|pair| pair[0] < pair[1]),
            "migrations are not strictly ordered"
        );
        assert!(versions[0] > 0, "versions start at 1");
    }

    #[test]
    fn checksum_is_stable_and_content_sensitive() {
        assert_eq!(checksum("select 1"), checksum("select 1"));
        assert_ne!(checksum("select 1"), checksum("select 1 "));
        // Known SHA-256 of "a".
        assert_eq!(
            checksum("a"),
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
        );
    }

    #[test]
    fn system_sql_is_non_empty_and_unqualified() {
        for migration in SYSTEM_MIGRATIONS {
            assert!(!migration.sql.trim().is_empty());
            // Tenant-safe: no *statement* may hard-code the `public` schema.
            // Comments are stripped first so prose mentioning `public.` is
            // not mistaken for a qualified identifier.
            let statements: String = migration
                .sql
                .lines()
                .map(|line| line.split_once("--").map_or(line, |(code, _)| code))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                !statements.contains("public."),
                "migration {} hard-codes the public schema",
                migration.name
            );
        }
    }
}
