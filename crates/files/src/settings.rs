//! The `storage` setting: where the file layer's configuration is kept.
//!
//! Storage is configured by the environment **and** overridable from the Studio, and the
//! override lives in `_settings` — one row, keyed `storage`, holding the same field names
//! the Studio's Files Storage page uses. One module reads and writes it, so the boot path
//! and the API cannot disagree about where it is.
//!
//! The secret inside it is stored encrypted ([`crate::secrets`]): `_settings` travels in
//! every backup, replica and dump, so a secret in the clear there would be a secret in all
//! of them.

use serde_json::{json, Value};

use lazypock_core::error::Result;
use lazypock_db::pool::db_error;
use lazypock_db::runner::QueryRunner;

/// The `_settings` key the storage configuration is stored under.
pub const STORAGE_SETTING: &str = "storage";

/// The stored setting, or `{}` when nothing has been saved.
///
/// An empty object rather than `None`: "nothing saved" and "saved as nothing" are the same
/// thing here, and every caller would immediately treat them the same way.
pub async fn storage_setting(runner: &dyn QueryRunner) -> Result<Value> {
    let row = runner
        .run_query_opt(
            "select value from _settings where key = $1::text",
            &[&STORAGE_SETTING.to_string()],
        )
        .await
        .map_err(db_error)?;

    Ok(row
        .map(|row| row.get::<_, Value>(0))
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({})))
}

/// Save the storage setting, replacing what was there.
pub async fn save_storage_setting(runner: &dyn QueryRunner, setting: &Value) -> Result<()> {
    runner
        .run_execute(
            "insert into _settings (key, value) values ($1::text, $2) \
             on conflict (key) do update set value = excluded.value, updated = now()",
            &[&STORAGE_SETTING.to_string(), setting],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}
