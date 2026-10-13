//! Running statements on either a pooled connection or a transaction.
//!
//! A record write has to work against a pool checkout (`Client`) and inside the
//! batch endpoint's transaction (`Transaction`). Rather than making every call
//! site generic, both are exposed through one object-safe trait.
//!
//! The futures are boxed and explicitly `Send` rather than using `async fn` in
//! the trait: an `async fn` trait method returns an opaque future whose
//! `Send`-ness is not declared, which would make the axum handler futures fail
//! to satisfy their `Send` bound. One allocation per statement is nothing next
//! to a database round trip.

use std::future::Future;
use std::pin::Pin;

use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Error, Row, Transaction};

/// A boxed, `Send` future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Something that can execute statements.
///
/// `Send + Sync` are supertraits on purpose: without them `&dyn QueryRunner`
/// would not be `Send`, and every axum handler that awaits through it would fail
/// its `Send` bound.
pub trait QueryRunner: Send + Sync {
    /// Like `Client::query_one`.
    fn run_query_one<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Row, Error>>;

    /// Like `Client::query_opt`.
    fn run_query_opt<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Option<Row>, Error>>;

    /// Like `Client::query`.
    fn run_query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Vec<Row>, Error>>;

    /// Like `Client::execute`.
    fn run_execute<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<u64, Error>>;

    /// Like `Client::batch_execute` (no parameters).
    fn run_batch_execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), Error>>;
}

impl QueryRunner for Client {
    fn run_query_one<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Row, Error>> {
        Box::pin(async move { self.query_one(sql, params).await })
    }

    fn run_query_opt<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Option<Row>, Error>> {
        Box::pin(async move { self.query_opt(sql, params).await })
    }

    fn run_query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Vec<Row>, Error>> {
        Box::pin(async move { self.query(sql, params).await })
    }

    fn run_execute<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<u64, Error>> {
        Box::pin(async move { self.execute(sql, params).await })
    }

    fn run_batch_execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), Error>> {
        Box::pin(async move { self.batch_execute(sql).await })
    }
}

/// The pooled client, so `&pooled_client` coerces straight to `&dyn QueryRunner`
/// without the caller having to deref by hand.
impl QueryRunner for deadpool_postgres::Client {
    fn run_query_one<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Row, Error>> {
        let inner: &Client = self;
        Box::pin(async move { inner.query_one(sql, params).await })
    }

    fn run_query_opt<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Option<Row>, Error>> {
        let inner: &Client = self;
        Box::pin(async move { inner.query_opt(sql, params).await })
    }

    fn run_query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Vec<Row>, Error>> {
        let inner: &Client = self;
        Box::pin(async move { inner.query(sql, params).await })
    }

    fn run_execute<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<u64, Error>> {
        let inner: &Client = self;
        Box::pin(async move { inner.execute(sql, params).await })
    }

    fn run_batch_execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), Error>> {
        let inner: &Client = self;
        Box::pin(async move { inner.batch_execute(sql).await })
    }
}

/// `deadpool-postgres` wraps the driver's transaction so it can hand the
/// connection back to the pool on commit; like the pooled client, it is covered
/// so `&transaction` coerces straight to `&dyn QueryRunner`.
impl QueryRunner for deadpool_postgres::Transaction<'_> {
    fn run_query_one<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Row, Error>> {
        let inner: &Transaction<'_> = self;
        Box::pin(async move { inner.query_one(sql, params).await })
    }

    fn run_query_opt<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Option<Row>, Error>> {
        let inner: &Transaction<'_> = self;
        Box::pin(async move { inner.query_opt(sql, params).await })
    }

    fn run_query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Vec<Row>, Error>> {
        let inner: &Transaction<'_> = self;
        Box::pin(async move { inner.query(sql, params).await })
    }

    fn run_execute<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<u64, Error>> {
        let inner: &Transaction<'_> = self;
        Box::pin(async move { inner.execute(sql, params).await })
    }

    fn run_batch_execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), Error>> {
        let inner: &Transaction<'_> = self;
        Box::pin(async move { inner.batch_execute(sql).await })
    }
}

impl QueryRunner for Transaction<'_> {
    fn run_query_one<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Row, Error>> {
        Box::pin(async move { self.query_one(sql, params).await })
    }

    fn run_query_opt<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Option<Row>, Error>> {
        Box::pin(async move { self.query_opt(sql, params).await })
    }

    fn run_query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<Vec<Row>, Error>> {
        Box::pin(async move { self.query(sql, params).await })
    }

    fn run_execute<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&(dyn ToSql + Sync)],
    ) -> BoxFuture<'a, Result<u64, Error>> {
        Box::pin(async move { self.execute(sql, params).await })
    }

    fn run_batch_execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), Error>> {
        Box::pin(async move { self.batch_execute(sql).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::TypeId;

    #[test]
    fn boxed_futures_are_send() {
        // Compile-time proof: a handler future containing these must be `Send`.
        fn assert_send<T: Send>(_: &T) {}
        let future: BoxFuture<'static, ()> = Box::pin(async {});
        assert_send(&future);
    }

    #[test]
    fn the_trait_is_object_safe() {
        // The whole point: one `&dyn` can accept either implementation.
        fn _takes(_: &dyn QueryRunner) {}
        assert_ne!(
            TypeId::of::<fn(&Client)>(),
            TypeId::of::<fn(&Transaction<'_>)>()
        );
    }
}
