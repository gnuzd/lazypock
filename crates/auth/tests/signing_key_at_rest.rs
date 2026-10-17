//! `LAZYPOCK_SECRET_KEY`: the signing key encrypted at rest.
//!
//! **Its own test binary on purpose.** Both variables involved are process-wide, so a
//! test that sets one would change what every other test in the same binary observes.
//! Cargo runs each integration test file as a separate process, which is the isolation
//! this needs — and within the binary the tests take a lock, because they still share a
//! process and each one changes the key that is set.
//!
//! The property under test: a signing key in `_settings` is a signing key in every dump,
//! replica and SQL console, and it mints a token for any account — superusers included.
//! Encrypting it with a key the environment holds is what separates "a leaked dump" from
//! "an app takeover", and the migration of an older plaintext key has to be a rewrite
//! rather than a rewrite-or-regenerate, which would sign everybody out.

mod support;

use tokio::sync::Mutex;

use base64::Engine;
use lazypock_auth::token::SigningKeys;
use lazypock_auth::AuthService;
use support::migrated_test_db;

/// Serialises the tests in this binary, which all set the same variable.
static ENV: Mutex<()> = Mutex::const_new(());

const SECRET_KEY: &str = "LAZYPOCK_SECRET_KEY";

/// The stored signing-key setting, if any.
async fn stored_key(test: &support::TestDb) -> Option<serde_json::Value> {
    let client = test.client().await;
    client
        .query_opt(
            "select value from _settings where key = 'auth.signing_key'",
            &[],
        )
        .await
        .expect("query")
        .map(|row| row.get("value"))
}

/// The stored `der` field, which is either base64 or ciphertext.
async fn stored_der(test: &support::TestDb) -> Option<String> {
    stored_key(test).await.and_then(|value| {
        value
            .get("der")
            .and_then(|der| der.as_str())
            .map(str::to_string)
    })
}

fn clear_env() {
    std::env::remove_var(SECRET_KEY);
    std::env::remove_var("LAZYPOCK_SIGNING_KEY");
}

/// A generated key is written encrypted, and reads back as the same key.
#[tokio::test]
async fn a_generated_key_is_stored_encrypted_and_reads_back() {
    let _guard = ENV.lock().await;
    clear_env();
    let Some(test) = migrated_test_db("signing_key_at_rest_generate").await else {
        return;
    };

    std::env::set_var(SECRET_KEY, "ab".repeat(32));
    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load");
    let kid = service.keys().expect("keys").kid().to_string();
    clear_env();

    let stored = stored_der(&test).await.expect("a stored key");
    assert!(
        stored.starts_with("v1:"),
        "the key must be ciphertext: {stored}"
    );
    assert!(
        !stored.contains(
            &base64::engine::general_purpose::STANDARD
                .encode(service.keys().expect("keys").to_der())
        ),
        "the plaintext key must not survive in _settings"
    );

    // The same variable finds the same key: a restart is not a new signer.
    std::env::set_var(SECRET_KEY, "ab".repeat(32));
    let again = AuthService::new(test.db.clone());
    again.load().await.expect("load again");
    clear_env();
    assert_eq!(again.keys().expect("keys").kid(), kid);

    test.cleanup().await;
}

/// A key written by an older version in plaintext is read, then rewritten encrypted.
#[tokio::test]
async fn a_plaintext_key_is_rewritten_encrypted_on_load() {
    let _guard = ENV.lock().await;
    clear_env();
    let Some(test) = migrated_test_db("signing_key_at_rest_upgrade").await else {
        return;
    };

    // What a previous version wrote: base64 of the DER, no encryption.
    let keys = SigningKeys::generate().expect("generate");
    let kid = keys.kid().to_string();
    let plaintext = base64::engine::general_purpose::STANDARD.encode(keys.to_der());
    test.client()
        .await
        .execute(
            "insert into _settings (key, value) values ('auth.signing_key', $1::text::jsonb)",
            &[&serde_json::json!({ "kid": kid, "der": plaintext }).to_string()],
        )
        .await
        .expect("seed a plaintext key");

    std::env::set_var(SECRET_KEY, "cd".repeat(32));
    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load");
    clear_env();

    assert_eq!(
        service.keys().expect("keys").kid(),
        kid,
        "the existing key must still sign: an upgrade is not a rotation"
    );
    let stored = stored_der(&test).await.expect("a stored key");
    assert!(
        stored.starts_with("v1:"),
        "the plaintext copy must have been rewritten: {stored}"
    );

    test.cleanup().await;
}

/// Without the variable the key stays plaintext — the old behaviour — rather than
/// silently changing how it is stored.
#[tokio::test]
async fn without_the_secret_key_the_key_stays_plaintext() {
    let _guard = ENV.lock().await;
    clear_env();
    let Some(test) = migrated_test_db("signing_key_at_rest_plaintext").await else {
        return;
    };

    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load");

    let stored = stored_der(&test).await.expect("a stored key");
    assert!(
        !stored.starts_with("v1:"),
        "no key configured means no encryption, not a broken value: {stored}"
    );
    // Still a working key, which is the whole reason the fallback exists.
    assert_eq!(
        service.keys().expect("keys").kid(),
        stored_key(&test).await.expect("value")["kid"]
            .as_str()
            .expect("kid")
    );

    test.cleanup().await;
}

/// A key encrypted under another secret cannot be read, and says which variable is wrong
/// instead of regenerating (which would sign everybody out and look like a token bug).
#[tokio::test]
async fn another_secret_key_is_refused_by_name_and_changes_nothing() {
    let _guard = ENV.lock().await;
    clear_env();
    let Some(test) = migrated_test_db("signing_key_at_rest_wrong_key").await else {
        return;
    };

    std::env::set_var(SECRET_KEY, "ab".repeat(32));
    AuthService::new(test.db.clone())
        .load()
        .await
        .expect("load with the right key");
    clear_env();
    let stored = stored_key(&test).await.expect("a stored key");

    // A different secret key.
    std::env::set_var(SECRET_KEY, "ef".repeat(32));
    let error = AuthService::new(test.db.clone())
        .load()
        .await
        .expect_err("the wrong key cannot read it");
    clear_env();
    assert!(
        error.message().contains(SECRET_KEY),
        "the message must name the variable: {}",
        error.message()
    );
    assert_eq!(
        stored_key(&test).await.as_ref(),
        Some(&stored),
        "a refused load must not replace the stored key"
    );

    // And with no key at all, the value is equally unreadable — but the operator is told
    // which variable to set rather than getting a fresh signer behind their back.
    let error = AuthService::new(test.db.clone())
        .load()
        .await
        .expect_err("an encrypted key cannot be read without the key");
    assert!(
        error.message().contains(SECRET_KEY),
        "the message must name the variable: {}",
        error.message()
    );
    assert_eq!(
        stored_key(&test).await.as_ref(),
        Some(&stored),
        "no key must not mean a regenerated one"
    );

    test.cleanup().await;
}
