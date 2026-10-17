//! Integration tests for the authentication flows.

mod support;

use lazypock_auth::service::AuthService;
use lazypock_auth::sessions;
use lazypock_auth::superuser;
use lazypock_auth::token::Subject;

use support::migrated_test_db;

#[tokio::test]
async fn a_superuser_can_be_created_and_authenticated() {
    let Some(test) = migrated_test_db("auth_superuser_login").await else {
        return;
    };
    let client = test.client().await;

    assert_eq!(superuser::count(&client).await.expect("count"), 0);

    let account = superuser::create(&client, "Admin@Example.com", "s3cret-password")
        .await
        .expect("create");

    // The email is normalised and the password is never stored, nor recoverable
    // from what is stored.
    assert_eq!(account.email, "admin@example.com");
    assert!(
        account.password_hash.starts_with("$argon2"),
        "{}",
        account.password_hash
    );
    assert!(!account.password_hash.contains("s3cret-password"));
    assert!(!account.token_key.is_empty());
    assert_eq!(superuser::count(&client).await.expect("count"), 1);

    // A duplicate email is refused, case-insensitively.
    let err = superuser::create(&client, "ADMIN@example.com", "another-password")
        .await
        .expect_err("duplicate");
    assert_eq!(err.status(), 409, "{err}");

    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load keys");

    let outcome = service
        .authenticate_superuser("ADMIN@example.com", "s3cret-password")
        .await
        .expect("login");
    assert_eq!(outcome.subject, Subject::Superuser);
    assert_eq!(outcome.user_id, account.id);
    assert!(outcome.expires_in > 0);
    assert!(!outcome.refresh_token.is_empty());

    let claims = service.verify(&outcome.access_token).expect("verify");
    assert_eq!(claims.sub, account.id);
    assert!(claims.typ.is_superuser());
    assert_eq!(claims.tkey, account.token_key);

    // A wrong password and an unknown email are indistinguishable to a caller.
    let wrong = service
        .authenticate_superuser("admin@example.com", "nope")
        .await
        .expect_err("wrong password");
    let unknown = service
        .authenticate_superuser("ghost@example.com", "s3cret-password")
        .await
        .expect_err("unknown account");
    assert_eq!(wrong.status(), 401);
    assert_eq!(unknown.status(), 401);
    assert_eq!(
        wrong.message(),
        unknown.message(),
        "the two failures must not be distinguishable"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn refresh_rotates_and_detects_replay() {
    let Some(test) = migrated_test_db("auth_refresh").await else {
        return;
    };
    let client = test.client().await;
    superuser::create(&client, "a@example.com", "password1")
        .await
        .expect("create");
    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load keys");

    let first = service
        .authenticate_superuser("a@example.com", "password1")
        .await
        .expect("login");
    let second = service
        .refresh(&first.refresh_token, &[], None)
        .await
        .expect("first refresh");
    assert_ne!(
        first.refresh_token, second.refresh_token,
        "the refresh token must rotate"
    );

    let third = service
        .refresh(&second.refresh_token, &[], None)
        .await
        .expect("second refresh");
    assert_ne!(second.refresh_token, third.refresh_token);

    // Replaying a spent token is theft: it fails, and it kills the family so the
    // newest token stops working too.
    let replay = service
        .refresh(&first.refresh_token, &[], None)
        .await
        .expect_err("replay");
    assert_eq!(replay.status(), 401, "{replay}");

    let after = service
        .refresh(&third.refresh_token, &[], None)
        .await
        .expect_err("the family should have been revoked");
    assert_eq!(after.status(), 401, "{after}");

    test.cleanup().await;
}

#[tokio::test]
async fn unknown_and_expired_refresh_tokens_are_refused() {
    let Some(test) = migrated_test_db("auth_refresh_invalid").await else {
        return;
    };
    let client = test.client().await;
    superuser::create(&client, "a@example.com", "password1")
        .await
        .expect("create");
    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load keys");

    for bad in [
        "",
        "not-a-token",
        &sessions::generate_token().expect("token"),
    ] {
        let err = service
            .refresh(bad, &[], None)
            .await
            .expect_err("unknown token");
        assert_eq!(err.status(), 401, "{err}");
    }

    test.cleanup().await;
}

#[tokio::test]
async fn signing_keys_are_shared_between_nodes() {
    let Some(test) = migrated_test_db("auth_keys").await else {
        return;
    };
    let service = AuthService::new(test.db.clone());
    let other = AuthService::new(test.db.clone());

    service.load().await.expect("first load");
    // A second node loads the same stored key rather than generating its own,
    // otherwise the two would reject each other's tokens.
    other.load().await.expect("second load");
    assert_eq!(
        service.keys().expect("keys").kid(),
        other.keys().expect("keys").kid()
    );

    let token = service
        .sign(Subject::Superuser, "u1", "a@example.com", "", "", "tk")
        .expect("sign");
    assert!(
        other.verify(&token).is_ok(),
        "a token from one node must verify on another"
    );

    let jwks = other.jwks().expect("jwks");
    assert_eq!(jwks["keys"][0]["kid"], service.keys().unwrap().kid());
    assert_eq!(jwks["keys"][0]["kty"], "OKP");

    test.cleanup().await;
}

#[tokio::test]
async fn changing_a_password_ends_sessions_and_rotates_the_token_key() {
    let Some(test) = migrated_test_db("auth_token_key").await else {
        return;
    };
    let client = test.client().await;
    let account = superuser::create(&client, "a@example.com", "password1")
        .await
        .expect("create");
    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load keys");

    let outcome = service
        .authenticate_superuser("a@example.com", "password1")
        .await
        .expect("login");

    superuser::set_password(&client, &account.id, "password2")
        .await
        .expect("set password");

    // The old password stops working and the new one starts.
    assert!(service
        .authenticate_superuser("a@example.com", "password1")
        .await
        .is_err());
    let fresh = service
        .authenticate_superuser("a@example.com", "password2")
        .await
        .expect("new password");
    assert_ne!(
        service.verify(&fresh.access_token).unwrap().tkey,
        account.token_key,
        "the token key must rotate with the password"
    );

    // Existing sessions are ended, so the old refresh token is useless.
    let err = service
        .refresh(&outcome.refresh_token, &[], None)
        .await
        .expect_err("sessions must be revoked");
    assert_eq!(err.status(), 401, "{err}");

    // The already-issued access token still verifies until it expires: revocation
    // takes effect at refresh, and the token's own lifetime is the backstop.
    assert!(
        service.verify(&outcome.access_token).is_ok(),
        "access tokens are not checked against the database on every request"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn sessions_are_listed_and_purged() {
    let Some(test) = migrated_test_db("auth_sessions").await else {
        return;
    };
    let client = test.client().await;
    superuser::create(&client, "a@example.com", "password1")
        .await
        .expect("create");
    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load keys");

    for _ in 0..3 {
        service
            .authenticate_superuser("a@example.com", "password1")
            .await
            .expect("login");
    }

    let count: i64 = client
        .query_one("select count(*) from _sessions", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 3);

    // Nothing has expired yet.
    assert_eq!(sessions::purge_expired(&client).await.expect("purge"), 0);

    // Expire them by hand and purge.
    client
        .batch_execute("update _sessions set expires_at = now() - interval '1 day'")
        .await
        .expect("expire");
    assert_eq!(sessions::purge_expired(&client).await.expect("purge"), 3);

    test.cleanup().await;
}

#[tokio::test]
async fn revoking_a_family_stops_every_token_in_it() {
    let Some(test) = migrated_test_db("auth_family_revoke").await else {
        return;
    };
    let client = test.client().await;

    let (first, session) = sessions::issue(&client, "", "u1", None, None)
        .await
        .expect("issue");
    let (second, second_session) = sessions::rotate(&client, &first, None, None)
        .await
        .expect("rotate");

    assert_eq!(
        session.family_id, second_session.family_id,
        "rotation must keep the family"
    );

    let revoked = sessions::revoke_family(&client, &session.family_id)
        .await
        .expect("revoke");
    assert_eq!(revoked, 1, "only the live token should need revoking");

    for token in [&first, &second] {
        let err = sessions::rotate(&client, token, None, None)
            .await
            .expect_err("a revoked token must not rotate");
        assert_eq!(err.status(), 401);
    }

    test.cleanup().await;
}

/// Rotation is claimed with one conditional statement, so two simultaneous uses of the
/// same token cannot both mint a token.
///
/// Before this, both calls read `revoked = false`, both updated, and both issued — which
/// quietly defeated the replay detection above: a thief racing the victim's own refresh
/// got a working token and no family revocation. The loser now looks exactly like the
/// replay it is, which means the family is revoked: a client must not refresh
/// concurrently, and that is the price of the detection actually working.
#[tokio::test]
async fn two_simultaneous_rotations_cannot_both_succeed() {
    let Some(test) = migrated_test_db("auth_refresh_race").await else {
        return;
    };
    let client = test.client().await;
    superuser::create(&client, "a@example.com", "password1")
        .await
        .expect("create");
    let service = AuthService::new(test.db.clone());
    service.load().await.expect("load keys");

    let login = service
        .authenticate_superuser("a@example.com", "password1")
        .await
        .expect("login");

    let (left, right) = tokio::join!(
        service.refresh(&login.refresh_token, &[], None),
        service.refresh(&login.refresh_token, &[], None),
    );
    assert!(
        left.is_ok() ^ right.is_ok(),
        "exactly one rotation may claim the token: {:?} / {:?}",
        left.as_ref().map(|_| "ok"),
        right.as_ref().map(|_| "ok")
    );

    // The loser was a replay, so the family is gone — including the winner's new token.
    let winner = left.or(right).expect("one rotation won");
    let after = service
        .refresh(&winner.refresh_token, &[], None)
        .await
        .expect_err("the replay revoked the family");
    assert_eq!(after.status(), 401, "{after}");

    test.cleanup().await;
}
