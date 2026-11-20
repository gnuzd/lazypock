// //! End-to-end auth tests against a real Postgres.
// //!
// //!   TEST_DATABASE_URL=postgres://owner@host:5432/EMPTY_DB cargo test -p lazypock-auth --test integration
// //!
// //! Same conventions as the db crate's integration test: an empty database, a role
// //! that may CREATE ROLE, and (to prove the pooler claims) a PgBouncer in
// //! transaction mode in front. Skipped when TEST_DATABASE_URL is unset.
//
// use lazypock_auth::{password, sessions, superuser, AuthService, ClientMeta, Subject};
// use lazypock_core::secret::SecretCipher;
// use lazypock_core::Error;
// use lazypock_db::{migrate, rls, Collection, CollectionType, Db, DbOptions, Field, FieldType, Ident, SchemaStore};
// use std::sync::Arc;
//
// const CIPHER_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
//
// fn cipher() -> Option<SecretCipher> {
//     Some(SecretCipher::parse(CIPHER_KEY).unwrap())
// }
//
// async fn owner_sql(db: &Db, sql: &str) {
//     let mut conn = db.conn().await.unwrap();
//     let tx = rls::begin_owner(&mut conn).await.unwrap();
//     tx.batch_execute(sql).await.unwrap();
//     tx.commit().await.unwrap();
// }
//
// async fn setting(db: &Db) -> Option<String> {
//     let conn = db.conn().await.unwrap();
//     let rows = conn
//         .query_typed("select value->>'der' from _settings where key = 'auth.signing_key'", &[])
//         .await
//         .unwrap();
//     rows.first().map(|r| r.get(0))
// }
//
// fn is_unauthorized<T: std::fmt::Debug>(r: Result<T, Error>, needle: &str) {
//     match r {
//         Err(Error::Unauthorized(m)) => assert!(m.contains(needle), "{m:?} should mention {needle:?}"),
//         other => panic!("expected Unauthorized({needle:?}), got {other:?}"),
//     }
// }
//
// #[tokio::test]
// async fn full_flow() {
//     let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
//         eprintln!("TEST_DATABASE_URL not set: skipping");
//         return;
//     };
//     let db = Db::connect(&url, &DbOptions { pool_size: 6, direct_url: std::env::var("TEST_DATABASE_DIRECT_URL").ok(), ..DbOptions::default() })
//         .await
//         .unwrap();
//     migrate::run(&db, migrate::SYSTEM).await.unwrap();
//     for sql in ["delete from _sessions", "delete from _superusers", "delete from _settings where key = 'auth.signing_key'"] {
//         owner_sql(&db, sql).await;
//     }
//
//     // ---- signing key: created once, encrypted at rest, shared by every node ---
//     let first = AuthService::new(db.clone(), cipher());
//     assert!(first.keys().is_err(), "not usable before load()");
//     first.load().await.unwrap();
//     let stored = setting(&db).await.unwrap();
//     assert!(stored.starts_with("v1:"), "the key is encrypted at rest: {stored}");
//     assert!(!stored.contains(&first.keys().unwrap().kid().to_string()));
//     let second = AuthService::new(db.clone(), cipher());
//     second.load().await.unwrap();
//     assert_eq!(first.keys().unwrap().kid(), second.keys().unwrap().kid(), "a second node adopts the stored key");
//     let token = first.sign_probe();
//     assert!(second.verify(&token).is_ok(), "a token from one node verifies on another");
//     let no_cipher = AuthService::new(db.clone(), None).load().await;
//     assert!(matches!(no_cipher, Err(Error::Config(_))), "an encrypted key cannot be read without LAZYPOCK_SECRET_KEY: {no_cipher:?}");
//
//     // plaintext -> encrypted upgrade keeps the same key
//     owner_sql(&db, "delete from _settings where key = 'auth.signing_key'").await;
//     let plain = AuthService::new(db.clone(), None);
//     plain.load().await.unwrap();
//     assert!(!setting(&db).await.unwrap().starts_with("v1:"), "no cipher: stored as plaintext (and warned about)");
//     let kid = plain.keys().unwrap().kid().to_string();
//     let upgraded = AuthService::new(db.clone(), cipher());
//     upgraded.load().await.unwrap();
//     assert!(setting(&db).await.unwrap().starts_with("v1:"), "upgraded in place");
//     assert_eq!(upgraded.keys().unwrap().kid(), kid);
//
//     // several nodes booting at once: the first insert wins, everyone agrees
//     owner_sql(&db, "delete from _settings where key = 'auth.signing_key'").await;
//     let nodes: Vec<Arc<AuthService>> = (0..5).map(|_| Arc::new(AuthService::new(db.clone(), cipher()))).collect();
//     let handles: Vec<_> = nodes.iter().cloned().map(|n| tokio::spawn(async move { n.load().await })).collect();
//     for h in handles {
//         h.await.unwrap().unwrap();
//     }
//     let kids: Vec<String> = nodes.iter().map(|n| n.keys().unwrap().kid().to_string()).collect();
//     assert!(kids.iter().all(|k| *k == kids[0]), "a boot race must not split the key: {kids:?}");
//     let auth = AuthService::new(db.clone(), cipher());
//     auth.load().await.unwrap();
//     assert_eq!(auth.keys().unwrap().kid(), kids[0]);
//     assert_eq!(auth.jwks().unwrap()["keys"][0]["kid"], kids[0]);
//     let meta = ClientMeta { user_agent: Some("tests".into()), ip: Some("203.0.113.9".into()) };
//
//     // ---- superuser --------------------------------------------------------
//     assert!(auth.ensure_superuser("Admin@Example.com", "correct horse").await.unwrap());
//     assert!(!auth.ensure_superuser("other@example.com", "correct horse").await.unwrap(), "only when none exist");
//     let bad_email = auth.authenticate_superuser("nobody@example.com", "whatever12", &meta).await;
//     let bad_pass = auth.authenticate_superuser("admin@example.com", "wrong password", &meta).await;
//     is_unauthorized(bad_email, "invalid email or password");
//     is_unauthorized(bad_pass, "invalid email or password"); // identical message: no account enumeration
//     let su = auth.authenticate_superuser(" ADMIN@example.com ", "correct horse", &meta).await.unwrap();
//     assert_eq!(su.subject, Subject::Superuser);
//     let claims = auth.verify(&su.access_token).unwrap();
//     assert!(claims.typ.is_superuser() && claims.col.is_empty());
//     let schema_store = SchemaStore::new(db.clone());
//     schema_store.reload().await.unwrap();
//     let snapshot = schema_store.cache().snapshot();
//     assert_eq!(auth.context_from_claims(&claims).role, lazypock_db::Role::Owner);
//     assert_eq!(auth.request_context(&claims, &snapshot).await.unwrap().role, lazypock_db::Role::Owner);
//     assert!(format!("{su:?}").find(&su.access_token).is_none() && format!("{su:?}").find(&su.refresh_token).is_none(), "tokens never reach Debug output");
//     {
//         let conn = db.conn().await.unwrap();
//         let admin = superuser::find_by_email(&**conn, "admin@example.com").await.unwrap().unwrap();
//         superuser::set_password(&**conn, &admin.id, "a brand new password").await.unwrap();
//     }
//     is_unauthorized(auth.request_context(&claims, &snapshot).await, "revoked");
//     is_unauthorized(auth.refresh(&su.refresh_token, &snapshot, None, &meta).await, "");
//     assert!(auth.authenticate_superuser("admin@example.com", "a brand new password", &meta).await.is_ok());
//     assert!(matches!(superuser::create(&**db.conn().await.unwrap(), "ADMIN@example.com", "correct horse").await, Err(Error::Conflict(_))));
//
//     // ---- an auth collection ----------------------------------------------
//     let _ = schema_store.delete(&Ident::new("auth_users_it").unwrap()).await;
//     let mut name = Field::new("name", FieldType::Text).unwrap();
//     name.required = false;
//     let mut users = Collection::new("auth_users_it", CollectionType::Auth).unwrap();
//     users.fields = vec![name];
//     let users = schema_store.create(&users).await.unwrap();
//     let hash = password::hash("hunter2 hunter2").unwrap();
//     owner_sql(&db, &format!("insert into auth_users_it (id, email, password_hash, token_key, name) values ('u1', 'Ann@Example.com', '{hash}', 'tk-1', 'Ann')")).await;
//     let snapshot = schema_store.cache().snapshot();
//     let base = {
//         let mut b = Collection::new("plain_it", CollectionType::Base).unwrap();
//         b.fields = vec![];
//         b
//     };
//
//     let login = auth.authenticate_record(&users, "ann@example.COM", "hunter2 hunter2", &meta).await.unwrap();
//     assert_eq!((login.subject, login.user_id.as_str(), login.collection_name.as_str()), (Subject::Auth, "u1", "auth_users_it"));
//     is_unauthorized(auth.authenticate_record(&users, "ann@example.com", "nope nope nope", &meta).await, "invalid email or password");
//     is_unauthorized(auth.authenticate_record(&users, "ghost@example.com", "hunter2 hunter2", &meta).await, "invalid email or password");
//     assert!(matches!(auth.authenticate_record(&base, "a@b.c", "x", &meta).await, Err(Error::Validation(_))), "not an auth collection");
//
//     let claims = auth.verify(&login.access_token).unwrap();
//     assert_eq!((claims.sub.as_str(), claims.col.as_str(), claims.email.as_str()), ("u1", users.id.as_str(), "Ann@Example.com"));
//     let ctx = auth.request_context(&claims, &snapshot).await.unwrap();
//     assert_eq!(ctx.role, lazypock_db::Role::Authenticated);
//     assert_eq!((ctx.auth_id.as_str(), ctx.auth_collection_name.as_str()), ("u1", "auth_users_it"));
//     let claims_json: serde_json::Value = serde_json::from_str(&ctx.auth_claims).unwrap();
//     assert_eq!(claims_json["name"], "Ann");
//     assert!(claims_json.get("password_hash").is_none() && claims_json.get("token_key").is_none(), "credentials never become rule claims: {claims_json}");
//
//     // refresh rotates; replaying an old token burns the family
//     let rotated = auth.refresh(&login.refresh_token, &snapshot, Some("auth_users_it"), &meta).await.unwrap();
//     assert_ne!(rotated.refresh_token, login.refresh_token);
//     assert_eq!(auth.verify(&rotated.access_token).unwrap().sub, "u1");
//     is_unauthorized(auth.refresh(&login.refresh_token, &snapshot, None, &meta).await, "already been used");
//     is_unauthorized(auth.refresh(&rotated.refresh_token, &snapshot, None, &meta).await, "already been used");
//     is_unauthorized(auth.refresh("not-a-token", &snapshot, None, &meta).await, "not recognised");
//
//     // a refresh token is pinned to its collection when the endpoint says so
//     let l2 = auth.authenticate_record(&users, "ann@example.com", "hunter2 hunter2", &meta).await.unwrap();
//     is_unauthorized(auth.refresh(&l2.refresh_token, &snapshot, Some("_superusers"), &meta).await, "different collection");
//     is_unauthorized(auth.refresh(&l2.refresh_token, &snapshot, Some("someone_else"), &meta).await, "different collection");
//     let ok = auth.refresh(&l2.refresh_token, &snapshot, Some("auth_users_it"), &meta).await.unwrap();
//     auth.logout(&ok.refresh_token).await.unwrap();
//     auth.logout("unknown-token").await.unwrap();
//     is_unauthorized(auth.refresh(&ok.refresh_token, &snapshot, None, &meta).await, "already been used");
//
//     // sign out everywhere
//     let (a, b) = (
//         auth.authenticate_record(&users, "ann@example.com", "hunter2 hunter2", &meta).await.unwrap(),
//         auth.authenticate_record(&users, "ann@example.com", "hunter2 hunter2", &meta).await.unwrap(),
//     );
//     assert_eq!(auth.revoke_all(&users.id, "u1").await.unwrap(), 2);
//     is_unauthorized(auth.refresh(&a.refresh_token, &snapshot, None, &meta).await, "");
//     is_unauthorized(auth.refresh(&b.refresh_token, &snapshot, None, &meta).await, "");
//
//     // sessions without a password (OAuth/OTP) and unknown accounts
//     let via = auth.issue_for_record(&users, "u1", &meta).await.unwrap();
//     assert_eq!(auth.verify(&via.access_token).unwrap().sub, "u1");
//     is_unauthorized(auth.issue_for_record(&users, "missing", &meta).await, "no longer exists");
//
//     // rotating the account's token key cuts off access tokens that were already issued
//     owner_sql(&db, "update auth_users_it set token_key = 'tk-2' where id = 'u1'").await;
//     assert!(auth.verify(&login.access_token).is_ok(), "signature and expiry alone cannot know");
//     is_unauthorized(auth.request_context(&claims, &snapshot).await, "revoked");
//     owner_sql(&db, "delete from auth_users_it where id = 'u1'").await;
//     is_unauthorized(auth.request_context(&claims, &snapshot).await, "no longer exists");
//
//     // a token for a collection that was deleted
//     schema_store.delete(&Ident::new("auth_users_it").unwrap()).await.unwrap();
//     let after = schema_store.cache().snapshot();
//     is_unauthorized(auth.request_context(&claims, &after).await, "collection no longer exists");
//
//     // ---- housekeeping ------------------------------------------------------
//     owner_sql(&db, "update _sessions set expires_at = now() - interval '1 day'").await;
//     assert!(sessions::purge_expired(&**db.conn().await.unwrap()).await.unwrap() >= 1);
//
//     // ---- pooler safety ----------------------------------------------------
//     let conn = db.conn().await.unwrap();
//     let n: i64 = conn.query_typed("select count(*) from pg_prepared_statements", &[]).await.unwrap()[0].get(0);
//     assert_eq!(n, 0, "no named prepared statements left on the session");
// }
//
// trait SignProbe {
//     fn sign_probe(&self) -> String;
// }
// impl SignProbe for AuthService {
//     fn sign_probe(&self) -> String {
//         let keys = self.keys().unwrap();
//         keys.sign(&keys.claims_for(Subject::Auth, "x", "x@y.z", "c", "n", "k").unwrap()).unwrap()
//     }
// }
