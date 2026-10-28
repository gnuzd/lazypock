//! The API's side of `$app`: the queries and the job queue.
//!
//! The JavaScript side decides *whether* an operation may happen — same-collection
//! writes are refused there, so a hook cannot re-enter its own collection. This side
//! decides *what it may see and touch*: every query opens a transaction, switches to
//! the transaction-scoped context the triggering request built, and only then runs.
//! `lazypock_anon` and `lazypock_authenticated` are what the generated policies are
//! written against, so an anonymous write's hook sees the public rows and nothing
//! else — the connecting role, which owns the tables, is never switched to.
//!
//! Three decisions worth naming:
//!
//! * **A write through `$app` does not fire hooks.** `records::create`/`update` are
//!   the no-hooks variants, so recursion is impossible rather than bounded — no depth
//!   counter to get wrong, and no half-written record behind a nested failure.
//! * **An auth collection's password is hashed here**, through the same
//!   `auth_columns` the record endpoint uses, so `$app.save` cannot store a plaintext
//!   password.
//! * **A view is refused.** A view carries no Postgres policies, so `$app` reading
//!   one would bypass its rules; refusing is the only answer that is not a hole.
//!
//! The blocking bridge is deliberate: the hook worker is an ordinary thread, and the
//! JavaScript call has to be synchronous because this runtime has no job queue. A
//! transaction that fails is rolled back by being dropped.

use std::sync::Arc;

use lazypock_core::error::{Error, Result};
use lazypock_db::collections::Collection;
use lazypock_db::deadpool_postgres::{Client, Transaction};
use lazypock_db::hooks::{HookContext, NoHooks};
use lazypock_db::pool::{db_error, Db};
use lazypock_db::query::compile::FilterContext;
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::records::{self, RecordWriteError};
use lazypock_db::rls;
use lazypock_db::rls::context::{Action, RequestContext};
use lazypock_db::schema::SchemaCache;
use lazypock_db::types::CollectionType;
use lazypock_hooks::{AppHost, AppOp};
use serde_json::{json, Value};

use crate::jobs;
use crate::mail::{job_for, SendMail};

/// `$app`, backed by the pool, the schema cache and the job queue.
#[derive(Clone)]
pub struct HookAppHost {
    db: Db,
    schema: Arc<SchemaCache>,
    /// The mail templates in effect, so `mailer.send` refuses an unknown name with the
    /// same message the endpoint does.
    mail_templates: Arc<lazypock_mail::Templates>,
    /// The runtime whose pool the queries come from. The hook worker is not a
    /// runtime thread, so this is how a synchronous JavaScript call waits for an
    /// asynchronous query.
    runtime: tokio::runtime::Handle,
}

impl std::fmt::Debug for HookAppHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HookAppHost")
            .finish_non_exhaustive()
    }
}

impl HookAppHost {
    /// Build a host for a running node.
    pub fn new(
        db: Db,
        schema: Arc<SchemaCache>,
        mail_templates: Arc<lazypock_mail::Templates>,
    ) -> Result<Self> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|err| Error::config(format!("$app needs a Tokio runtime: {err}")))?;
        Ok(Self {
            db,
            schema,
            mail_templates,
            runtime,
        })
    }

    /// The collection and the filter context its rules compile against.
    fn resolve(&self, name: &str) -> Result<(Collection, FilterContext)> {
        let schema = self.schema.snapshot();
        let collection = schema.get(name).cloned().ok_or_else(|| {
            Error::invalid(format!("$app: there is no collection named {name:?}"))
        })?;
        let context = FilterContext::for_collection(&collection, schema.collections())?;
        Ok((collection, context))
    }

    /// A transaction running as the triggering request, for one action.
    async fn scoped<'a>(
        &self,
        client: &'a mut Client,
        request: &RequestContext,
    ) -> Result<Transaction<'a>> {
        let transaction = client.transaction().await.map_err(db_error)?;
        rls::context::apply(&transaction, request).await?;
        Ok(transaction)
    }

    async fn run(&self, context: &HookContext, op: AppOp) -> Result<Value> {
        match op {
            AppOp::FindRecordById { collection, id } => {
                self.find_record(context, &collection, &id).await
            }
            AppOp::Save { collection, record } => self.save(context, &collection, record).await,
            AppOp::Delete { collection, id } => self.delete(context, &collection, &id).await,
            AppOp::MailerSend { message } => self.send_mail(context, message).await,
        }
    }

    /// `$app.findRecordById` — one record, as the caller's rules allow seeing it.
    async fn find_record(
        &self,
        context: &HookContext,
        collection_name: &str,
        id: &str,
    ) -> Result<Value> {
        let (collection, filter) = self.resolve(collection_name)?;
        refuse_view(&collection, "read")?;

        // The same default projection the record endpoint uses, so hidden fields are
        // hidden here for the same reasons.
        let selection = FieldSelection::parse(None, &filter)?;
        let expand = ExpandSpec::parse(None, &filter)?;

        // A read is matched against `viewRule`, not against the action of whatever
        // write happens to be in flight.
        let request = context.request.clone().with_action(Action::View);
        let mut client = self.db.client().await?;
        let transaction = self.scoped(&mut client, &request).await?;
        let record =
            records::get_one(&transaction, &collection, &filter, id, &selection, &expand).await?;
        transaction.commit().await.map_err(db_error)?;

        // Absent and forbidden are the same answer: a record the rules do not allow
        // is a record the caller cannot tell apart from one that does not exist.
        Ok(record.unwrap_or(Value::Null))
    }

    /// `$app.save` — create, or update when the record carries an `id`.
    async fn save(
        &self,
        context: &HookContext,
        collection_name: &str,
        record: Value,
    ) -> Result<Value> {
        let (collection, filter) = self.resolve(collection_name)?;
        refuse_view(&collection, "write")?;
        if !record.is_object() {
            return Err(Error::invalid("$app.save: a record must be a JSON object"));
        }

        let id = record
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string);

        // An auth collection's password is hashed by `auth_columns`, before the
        // record layer ever sees it; without this a hook could store one in plain
        // text in the column the server compares hashes against.
        let extra = if collection.collection_type.is_auth() {
            crate::auth::validate_first(&collection, &record, id.is_some())
                .map_err(|err| err.error)?;
            crate::auth::auth_columns(&collection, &record).map_err(|err| err.error)?
        } else {
            Vec::new()
        };

        let action = if id.is_some() {
            Action::Update
        } else {
            Action::Write
        };
        let request = context.request.clone().with_action(action);
        let mut client = self.db.client().await?;
        let transaction = self.scoped(&mut client, &request).await?;

        // No hooks: a write made through `$app` must not re-enter the hook engine.
        let stored = match &id {
            Some(id) => records::update_with(
                &transaction,
                &collection,
                &filter,
                id,
                record,
                &extra,
                &NoHooks,
            )
            .await
            .map_err(write_error)?,
            None => Some(
                records::create_with(&transaction, &collection, &filter, record, &extra, &NoHooks)
                    .await
                    .map_err(write_error)?,
            ),
        };

        transaction.commit().await.map_err(db_error)?;
        stored.ok_or_else(|| {
            Error::invalid(format!(
                "$app.save: no record {:?} in {:?} that these rules allow updating",
                id.unwrap_or_default(),
                collection_name
            ))
        })
    }

    /// `$app.delete` — one record, matched against `deleteRule`.
    async fn delete(
        &self,
        context: &HookContext,
        collection_name: &str,
        id: &str,
    ) -> Result<Value> {
        let (collection, _filter) = self.resolve(collection_name)?;
        refuse_view(&collection, "delete")?;

        let request = context.request.clone().with_action(Action::Delete);
        let mut client = self.db.client().await?;
        let transaction = self.scoped(&mut client, &request).await?;
        let removed = records::delete(&transaction, &collection, id)
            .await
            .map_err(write_error)?;
        transaction.commit().await.map_err(db_error)?;

        Ok(json!({ "deleted": removed }))
    }

    /// `$app.mailer.send` — the same queue `POST /api/mail` writes to.
    ///
    /// Queued, never sent inline: SMTP inside a hook would hold a dispatch open for
    /// as long as the provider takes, and the hook deadline is two seconds.
    async fn send_mail(&self, context: &HookContext, message: Value) -> Result<Value> {
        let body: SendMail = serde_json::from_value(message)
            .map_err(|err| Error::invalid(format!("$app.mailer.send: {err}")))?;
        let client = self.db.client().await?;

        // The same names the endpoint resolves against: the database's bodies over the
        // files and the built-ins, so a hook naming a template that does not exist is
        // refused here rather than queued into a job that can only fail.
        let templates = crate::mail_templates::effective(&client, &self.mail_templates)
            .await
            .map_err(|err| err.error)?;
        let known = templates.names();
        let (job, template, to) = job_for(&body, &known).map_err(|err| err.error)?;

        // The same refusal the endpoint gives: a job that can only fail would retry
        // and be marked dead, and the hook would learn nothing.
        let settings = jobs::read_settings(&client)
            .await
            .map_err(Error::internal)?;
        lazypock_mail::MailConfig::from_settings(&settings)?.ok_or_else(|| {
            Error::invalid("mail is not configured: set it in Studio → Settings → Mail")
        })?;

        let job_id = jobs::enqueue(&client, job).await.map_err(|err| err.error)?;

        // Who queued it: the hook acts for whoever triggered the write, and an
        // anonymous trigger is recorded as such rather than as the server.
        let actor = if context.request.auth_id.is_empty() {
            "hook:anonymous"
        } else {
            context.request.auth_id.as_str()
        };
        crate::audit::record(
            &client,
            actor,
            crate::audit::action::MAIL_SEND,
            Some(&to),
            json!({ "template": template, "job": job_id, "via": "hook" }),
        )
        .await?;

        Ok(json!({ "queued": true, "id": job_id }))
    }
}

impl AppHost for HookAppHost {
    fn call(&self, context: &HookContext, op: AppOp) -> Result<Value> {
        let context = context.clone();
        // Blocks this hook worker thread on the async work. Legal (it is not a
        // runtime thread) and intentional: the JavaScript call it answers cannot
        // await anything.
        self.runtime.block_on(self.run(&context, op))
    }
}

/// A view has no policies, so `$app` cannot run under the caller's rules on it.
fn refuse_view(collection: &Collection, operation: &str) -> Result<()> {
    if collection.collection_type == CollectionType::View {
        return Err(Error::invalid(format!(
            "$app: {:?} is a view — it carries no policies, so it cannot be {operation} through \
             $app; use a table collection",
            collection.name
        )));
    }
    Ok(())
}

/// A failed write, as the error a hook sees.
///
/// The per-field detail is flattened into the message: a hook has no form to put it
/// next to, and the field names are what the hook author needs.
fn write_error(error: RecordWriteError) -> Error {
    match error {
        RecordWriteError::Validation(errors) => {
            Error::invalid(format!("Failed to save the record. {}", errors.summary()))
        }
        RecordWriteError::Other(error) => error,
    }
}
