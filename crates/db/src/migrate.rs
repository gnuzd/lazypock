//! Forward-only SQL migrations (same logic as the rest of lazypock).
//!
//! - Files live in the workspace's `migrations/` and are embedded at build time.
//! - Each migration runs in **its own transaction together with its ledger row**,
//!   so a failure leaves nothing half-applied and the next boot resumes there.
//! - Applied migrations are checksummed; editing one is an error.
//! - A database that has migrations this binary does not know is refused: the
//!   binary is older than the data, and running it could corrupt it.
//!
//! Locking is **per transaction** (`pg_advisory_xact_lock`), taken at the start of
//! each migration's transaction, which re-checks the ledger before applying. That
//! is correct on its own (two nodes booting together just serialise) and, unlike a
//! session-level lock held across several transactions, it works through a
//! transaction-mode pooler such as PgBouncer.

use crate::error::PgExt;
use crate::params::typed;
use crate::pool::Db;
use lazypock_core::Error;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use lazypock_core::Result;

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

macro_rules! migration {
    ($v:expr, $file:literal) => {
        Migration {
            version: $v,
            name: $file,
            sql: include_str!(concat!("../../../migrations/", $file, ".sql")),
        }
    };
}

/// Migrations that ship with the engine. Append only; never edit an applied one.
pub const SYSTEM: &[Migration] = &[
    migration!(1, "0001_system_tables"),
    migration!(2, "0002_superusers"),
    migration!(3, "0003_rls"),
    migration!(4, "0004_realtime"),
    migration!(5, "0005_image_presets"),
    migration!(6, "0006_auth_system_fields"),
    migration!(7, "0007_request_logs"),
    migration!(8, "0008_crons"),
    migration!(9, "0009_import_snapshots"),
    migration!(10, "0010_token_key_is_not_a_field"),
    migration!(11, "0011_files"),
    migration!(12, "0012_oauth2"),
    migration!(13, "0013_otps"),
    migration!(14, "0014_api_keys"),
    migration!(15, "0015_file_rls"),
    migration!(16, "0016_reflected_field_defaults"),
    migration!(17, "0017_auth_claims"),
    migration!(18, "0018_realtime_hardening"),
    migration!(19, "0019_grant_roles_to_owner"),
    migration!(20, "0020_revoke_system_tables"),
];

const LOCK_KEY: i64 = 0x6c70_6d69_6772; // "lpmigr"

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Versions applied by this call.
    pub applied: Vec<i64>,
    /// Highest version in the database afterwards.
    pub current: Option<i64>,
}

pub async fn run(db: &Db, migrations: &[Migration]) -> Result<Report> {
    validate(migrations)?;
    let mut conn = db.conn().await?;
    let client: &mut tokio_postgres::Client = &mut conn;

    // The ledger itself: created under the lock so two first boots cannot race.
    {
        let tx = client.transaction().await.pg()?;
        lock(&tx).await?;
        tx.batch_execute(
            "create table if not exists _migrations (
                 version    bigint primary key,
                 name       text not null,
                 checksum   text not null,
                 applied_at timestamptz not null default now()
             )",
        )
        .await
        .pg()?;
        tx.commit().await.pg()?;
    }

    let applied: HashMap<i64, String> = client
        .query_typed("select version, checksum from _migrations", &[])
        .await
        .pg()?
        .into_iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();

    let known: HashMap<i64, &Migration> = migrations.iter().map(|m| (m.version, m)).collect();
    let newest_known = migrations.last().map_or(0, |m| m.version);
    if let Some(unknown) = applied.keys().filter(|v| !known.contains_key(v)).max() {
        return Err(Error::Database(format!(
            "the database has migration {unknown} but this binary only knows up to {newest_known}: \
             it is newer than the binary — upgrade the binary"
        )));
    }
    for (v, sum) in &applied {
        if known[v].checksum() != *sum {
            return Err(Error::Database(format!(
                "migration {v} ({}) was changed after it was applied",
                known[v].name
            )));
        }
    }

    let mut report = Report::default();
    for m in migrations {
        if applied.contains_key(&m.version) {
            continue;
        }
        let tx = client.transaction().await.pg()?;
        lock(&tx).await?;
        // Another node may have applied it while we waited for the lock.
        let done = tx
            .query_typed(
                "select 1 from _migrations where version = $1",
                &[typed!(m.version, INT8)],
            )
            .await
            .pg()?;
        if !done.is_empty() {
            tx.rollback().await.pg()?;
            continue;
        }
        // Startup-time and operator-facing: keep the server's own message.
        tx.batch_execute(m.sql).await.map_err(|e| {
            Error::Database(format!(
                "migration {} ({}) failed: {}",
                m.version,
                m.name,
                e.as_db_error()
                    .map_or_else(|| e.to_string(), |d| d.message().to_owned())
            ))
        })?;
        let sum = m.checksum();
        tx.query_typed(
            "insert into _migrations (version, name, checksum) values ($1, $2, $3)",
            &[
                typed!(m.version, INT8),
                typed!(m.name, TEXT),
                typed!(sum, TEXT),
            ],
        )
        .await
        .pg()?;
        tx.commit().await.pg()?;
        tracing::info!(version = m.version, name = m.name, "migration applied");
        report.applied.push(m.version);
    }
    report.current = migrations.last().map(|m| m.version);
    Ok(report)
}

async fn lock(tx: &tokio_postgres::Transaction<'_>) -> Result<()> {
    tx.batch_execute(&format!("select pg_advisory_xact_lock({LOCK_KEY})"))
        .await
        .pg()
}

fn validate(migrations: &[Migration]) -> Result<()> {
    for m in migrations {
        if m.sql.trim().is_empty() {
            return Err(Error::Internal(format!("migration {} is empty", m.version)));
        }
    }
    for w in migrations.windows(2) {
        if w[0].version >= w[1].version {
            return Err(Error::Internal(format!(
                "migration versions must be strictly increasing ({} then {})",
                w[0].version, w[1].version
            )));
        }
    }
    Ok(())
}

impl Migration {
    pub fn checksum(&self) -> String {
        Sha256::digest(self.sql.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_migrations_are_valid() {
        validate(SYSTEM).unwrap();
        assert_eq!(SYSTEM.len(), 21);
    }

    #[test]
    fn every_file_in_migrations_is_registered_with_the_right_version() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations");
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".sql"))
            .collect();
        files.sort();
        let registered: Vec<String> = SYSTEM.iter().map(|m| format!("{}.sql", m.name)).collect();
        assert_eq!(
            files, registered,
            "a migration file is not registered in SYSTEM (or the reverse)"
        );
        for m in SYSTEM {
            let prefix: i64 = m.name[..4].parse().unwrap();
            assert_eq!(
                prefix, m.version,
                "{}: version does not match the file prefix",
                m.name
            );
        }
    }

    #[test]
    fn checksum_is_stable_hex() {
        let m = Migration {
            version: 1,
            name: "x",
            sql: "x",
        };
        assert_eq!(
            m.checksum(),
            "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881"
        );
    }

    #[test]
    fn out_of_order_or_empty_lists_are_refused() {
        let a = Migration {
            version: 2,
            name: "a",
            sql: "select 1",
        };
        let b = Migration {
            version: 1,
            name: "b",
            sql: "select 1",
        };
        assert!(validate(&[a, b]).is_err());
        assert!(
            validate(&[Migration {
                version: 1,
                name: "e",
                sql: "  "
            }])
            .is_err()
        );
    }
}
