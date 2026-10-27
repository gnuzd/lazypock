//! Mail templates stored in the database: `_mail_templates`.
//!
//! A deployment that wants to change an email either drops a file in the template
//! directory, which needs a deploy, or edits it in the Studio, which needs this
//! table. Resolution puts this layer first for that reason.
//!
//! Nothing is seeded. A row exists only once an operator saves something, so
//! "delete" is exactly "fall back to the file or the built-in" — and a release that
//! improves a built-in template is not silently shadowed by a copy of the old wording
//! taken the day the deployment was set up.
//!
//! The table is system-owned. `0003_rls.sql`'s default privileges hand every new
//! table to the two restricted roles; `0019_mail_templates.sql` revokes them again,
//! because a template is rendered with the variables of an auth email — a
//! verification or password-reset token, the recipient's address — so whoever can
//! write a template can mail themselves somebody else's token.

use lazypock_core::error::{Error, Result};

use crate::pool::db_error;
use crate::runner::QueryRunner;

/// The table the bodies live in.
pub const TABLE: &str = "_mail_templates";

/// The parts a template may have, in the order an editor shows them.
pub const PARTS: [&str; 3] = ["subject", "html", "text"];

/// The longest body accepted, in bytes.
///
/// Bounded because the body is stored in a row and parsed on every send: a megabyte
/// template is a mistake rather than a design.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// The longest name accepted.
pub const MAX_NAME_LEN: usize = 64;

/// One stored body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// The template name.
    pub name: String,
    /// `subject`, `html` or `text`.
    pub part: String,
    /// The body.
    pub body: String,
}

/// Every stored body, ordered by name then part.
pub async fn load(runner: &impl QueryRunner) -> Result<Vec<Stored>> {
    let rows = runner
        .run_query(
            "select name, part, body from _mail_templates order by name, part",
            &[],
        )
        .await
        .map_err(db_error)?;
    Ok(rows
        .iter()
        .map(|row| Stored {
            name: row.get(0),
            part: row.get(1),
            body: row.get(2),
        })
        .collect())
}

/// Write `parts` of one template, replacing any body already stored.
///
/// One statement rather than one per part, so a template cannot end up half saved —
/// rendering with yesterday's subject and today's body — without needing a
/// transaction, which a pooled checkout does not offer through [`QueryRunner`].
pub async fn save(runner: &impl QueryRunner, name: &str, parts: &[(&str, &str)]) -> Result<()> {
    if parts.is_empty() {
        return Err(Error::invalid("a mail template needs at least one part"));
    }
    let names: Vec<String> = parts.iter().map(|(part, _)| (*part).to_string()).collect();
    let bodies: Vec<String> = parts.iter().map(|(_, body)| (*body).to_string()).collect();

    runner
        .run_execute(
            "insert into _mail_templates (name, part, body) \
             select $1, saved.part, saved.body \
             from unnest($2::text[], $3::text[]) as saved(part, body) \
             on conflict (name, part) do update set body = excluded.body, updated = now()",
            &[&name, &names, &bodies],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Remove every stored body of `name`, answering how many rows that was.
///
/// Zero is not an error: deleting a template that only ever existed as a file or a
/// built-in is "leave it as the default", which is what the caller asked for.
pub async fn delete(runner: &impl QueryRunner, name: &str) -> Result<u64> {
    runner
        .run_execute("delete from _mail_templates where name = $1", &[&name])
        .await
        .map_err(db_error)
}
