//! `LAZYPOCK_SIGNING_KEY`: using a signing key from the environment.
//!
//! **Its own test binary on purpose.** The variable is process-wide, so a test that
//! sets it would change what every other test in the same binary observes. Cargo runs
//! each integration test file as a separate process, which is the isolation this
//! needs — and the reason it is not in `auth.rs` with the others.
//!
//! The property under test is not only "the key is used": it is that the key is
//! **never written to the database**, because a signing key in `_settings` is a
//! signing key in every dump, replica and SQL console, and it can forge a token for
//! anybody.

mod support;

use base64::Engine;
use lazypock_auth::token::SigningKeys;
use lazypock_auth::AuthService;
use support::migrated_test_db;

/// Whether a signing key is stored in the database.
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

#[tokio::test]
async fn a_key_from_the_environment_is_used_and_never_stored() {
    let Some(test) = migrated_test_db("signing_key_env").await else {
        return;
    };

    // A key this test controls, so the kid can be asserted exactly.
    let keys = SigningKeys::generate().expect("generate");
    let kid = keys.kid().to_string();
    let encoded = base64::engine::general_purpose::STANDARD.encode(keys.to_der());

    std::env::set_var("LAZYPOCK_SIGNING_KEY", &encoded);
    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load");
    std::env::remove_var("LAZYPOCK_SIGNING_KEY");

    assert_eq!(
        service.keys().expect("keys").kid(),
        kid,
        "the supplied key must be the one in use"
    );
    assert!(
        stored_key(&test).await.is_none(),
        "the key must not be written to the database when it came from the environment"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn a_32_byte_seed_is_accepted() {
    let Some(test) = migrated_test_db("signing_key_env_seed").await else {
        return;
    };

    // Exactly what `openssl rand -base64 32` produces: 32 raw bytes, base64-encoded.
    // The value below is a fixed seed so the expected key id is deterministic.
    let seed: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ];
    let encoded = base64::engine::general_purpose::STANDARD.encode(seed);
    let expected = SigningKeys::from_seed(&seed).expect("seed");

    // A value pasted from a wrapped line still works: whitespace is not content.
    std::env::set_var("LAZYPOCK_SIGNING_KEY", format!(" {} \n", encoded));
    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load");
    std::env::remove_var("LAZYPOCK_SIGNING_KEY");

    assert_eq!(
        service.keys().expect("keys").kid(),
        expected.kid(),
        "the 32-byte seed must be the key in use"
    );
    assert!(
        stored_key(&test).await.is_none(),
        "the key must not be written to the database when it came from the environment"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn an_empty_or_malformed_value_is_refused_by_name() {
    let Some(test) = migrated_test_db("signing_key_env_bad").await else {
        return;
    };

    // Set but empty: nearly always a templating mistake, and falling back to the
    // database would hide it until tokens quietly changed signer.
    std::env::set_var("LAZYPOCK_SIGNING_KEY", "   ");
    let service = AuthService::new(test.db.clone());
    let error = service.load().await.expect_err("an empty key is refused");
    std::env::remove_var("LAZYPOCK_SIGNING_KEY");
    assert!(
        error.message().contains("LAZYPOCK_SIGNING_KEY"),
        "the message must name the variable: {}",
        error.message()
    );

    // Not base64 — and the message must say how to produce a good one, because the
    // example files used to show the literal `change-me`, which lands here.
    std::env::set_var("LAZYPOCK_SIGNING_KEY", "change-me");
    let service = AuthService::new(test.db.clone());
    let error = service
        .load()
        .await
        .expect_err("a malformed key is refused");
    std::env::remove_var("LAZYPOCK_SIGNING_KEY");
    assert!(error.message().contains("base64"), "{}", error.message());
    assert!(
        error.message().contains("openssl rand -base64 32"),
        "the message must show how to generate a key: {}",
        error.message()
    );

    // Base64, but the wrong size and not a PKCS#8 document.
    std::env::set_var(
        "LAZYPOCK_SIGNING_KEY",
        base64::engine::general_purpose::STANDARD.encode([0u8; 16]),
    );
    let service = AuthService::new(test.db.clone());
    let error = service
        .load()
        .await
        .expect_err("a wrong-length key is refused");
    std::env::remove_var("LAZYPOCK_SIGNING_KEY");
    assert!(
        error.message().contains("16 bytes") && error.message().contains("openssl"),
        "{}",
        error.message()
    );

    // Neither attempt created a key behind the operator's back.
    assert!(
        stored_key(&test).await.is_none(),
        "a refused key must not leave a generated one behind"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn without_the_variable_the_database_is_still_the_default() {
    // The fallback is deliberate: removing it would make a restart with no key
    // configured silently invalidate every session.
    let Some(test) = migrated_test_db("signing_key_db_fallback").await else {
        return;
    };

    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load");
    let kid = service.keys().expect("keys").kid().to_string();

    assert!(
        stored_key(&test).await.is_some(),
        "with no environment key the generated one is stored, so nodes agree"
    );

    // And a second load picks up the same key rather than generating a new one.
    let again = AuthService::new(test.db.clone());
    again.load().await.expect("load again");
    assert_eq!(
        again.keys().expect("keys").kid(),
        kid,
        "a restart must not invalidate the tokens already issued"
    );

    test.cleanup().await;
}
