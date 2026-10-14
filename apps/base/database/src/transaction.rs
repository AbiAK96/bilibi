//! Explicit business-write boundary. Nested owners share this transaction;
//! never commit an inner transaction or publish before the outer commit.
use crate::{Error, Result};
use serde_json::{json, Value};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};
use tokio_postgres::{Client, Transaction};

pub type CaptureFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// Attribution is supplied by the trusted session/worker adapter, never form data.
/// Database does not decide tenancy or authorization; F3 supplies sealed proofs.
#[derive(Clone, Debug)]
pub struct Principal {
    pub kind: String,
    pub id: String,
}
impl Principal {
    pub fn user(id: i64) -> Self {
        Self {
            kind: "user".into(),
            id: id.to_string(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Context {
    pub tenant: i64,
    pub actor: Principal,
    pub impersonator: Option<Principal>,
    pub action: String,
    pub trace_id: String,
}
impl Context {
    pub fn user(tenant: i64, user: i64, action: impl Into<String>) -> Self {
        Self {
            tenant,
            actor: Principal::user(user),
            impersonator: None,
            action: action.into(),
            trace_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn validate(&self) -> Result<()> {
        if self.tenant <= 0
            || self.actor.kind.is_empty()
            || self.actor.id.is_empty()
            || self.action.is_empty()
            || self.trace_id.is_empty()
            || self
                .impersonator
                .as_ref()
                .is_some_and(|p| p.kind.is_empty() || p.id.is_empty())
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    fn value(&self) -> Value {
        let principal = |p: &Principal| json!({"kind":p.kind,"id":p.id});
        json!({"actor":principal(&self.actor),"impersonator":self.impersonator.as_ref().map(principal),"action":self.action,"trace_id":self.trace_id})
    }
}
#[derive(Debug)]
pub struct Change {
    pub kind: String,
    pub id: i64,
    pub old: Value,
    pub new: Value,
}

/// Audit owns redaction and persistence. Every implementation must use the
/// supplied transaction and return failures; capture cannot be best-effort.
pub trait Capture: Send + Sync {
    fn mutation<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        context: &'a Context,
        change: &'a Change,
    ) -> CaptureFuture<'a>;
    fn action<'a>(&'a self, tx: &'a Transaction<'_>, context: &'a Context) -> CaptureFuture<'a>;
}

/// Bridge for the original demo ledger. P5 can replace this via `begin_with`;
/// historical rows and migration IDs remain intact.
pub struct DemoCapture;
impl Capture for DemoCapture {
    fn mutation<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        context: &'a Context,
        change: &'a Change,
    ) -> CaptureFuture<'a> {
        Box::pin(async move {
            let actor = if context.actor.kind == "user" {
                context.actor.id.parse::<i64>().ok()
            } else {
                None
            };
            tx.execute("INSERT INTO audit_mutations(tenant_id,actor_id,auditable_type,auditable_id,old_values,new_values,trace_id,context) VALUES($1,$2,$3,$4,$5,$6,$7,$8)", &[&context.tenant,&actor,&change.kind,&change.id,&change.old,&change.new,&context.trace_id,&context.value()]).await?;
            Ok(())
        })
    }
    fn action<'a>(&'a self, tx: &'a Transaction<'_>, context: &'a Context) -> CaptureFuture<'a> {
        Box::pin(async move {
            tx.execute("INSERT INTO bilibi_action_commits(tenant_id,action,trace_id,context) VALUES($1,$2,$3,$4)", &[&context.tenant,&context.action,&context.trace_id,&context.value()]).await?;
            Ok(())
        })
    }
}

/// Shared registry service. Audit binds its capture once during installation;
/// the historical demo adapter keeps the current increment runnable beforehand.
#[derive(Default)]
pub struct MutationBoundary {
    capture: OnceLock<Arc<dyn Capture>>,
}
impl MutationBoundary {
    pub fn bind_capture(&self, capture: Arc<dyn Capture>) -> bilibi_module_registry::Result<()> {
        self.capture.set(capture).map_err(|_| {
            bilibi_module_registry::RegistryError::Service(
                "duplicate mutation capture binding".into(),
            )
        })
    }
    pub async fn begin<'a>(
        &self,
        client: &'a mut Client,
        context: Context,
    ) -> Result<MutationTransaction<'a>> {
        MutationTransaction::begin_with(
            client,
            context,
            self.capture
                .get()
                .cloned()
                .unwrap_or_else(|| Arc::new(DemoCapture)),
        )
        .await
    }
}

#[must_use = "commit explicitly; dropping rolls back and discards callbacks"]
pub struct MutationTransaction<'a> {
    tx: Transaction<'a>,
    context: Context,
    capture: Arc<dyn Capture>,
    callbacks: Vec<Box<dyn FnOnce() + Send>>,
    poisoned: bool,
}
impl<'a> MutationTransaction<'a> {
    pub async fn begin(client: &'a mut Client, context: Context) -> Result<Self> {
        Self::begin_with(client, context, Arc::new(DemoCapture)).await
    }
    pub async fn begin_with(
        client: &'a mut Client,
        context: Context,
        capture: Arc<dyn Capture>,
    ) -> Result<Self> {
        context.validate()?;
        Ok(Self {
            tx: client.transaction().await?,
            context,
            capture,
            callbacks: Vec::new(),
            poisoned: false,
        })
    }
    pub fn context(&self) -> &Context {
        &self.context
    }
    /// Owner SQL and locking participate in this transaction. Every business
    /// write must be paired with `record`; lifecycle SQL uses a separate seam.
    pub fn sql(&self) -> &Transaction<'a> {
        &self.tx
    }
    pub async fn record(&mut self, change: Change) -> Result<()> {
        if self.poisoned {
            return Err(Error::Internal);
        }
        if change.kind.is_empty() || change.id <= 0 {
            self.poisoned = true;
            return Err(Error::Invalid);
        }
        // Cancellation while awaiting capture must also forbid commit.
        self.poisoned = true;
        let result = self
            .capture
            .mutation(&self.tx, &self.context, &change)
            .await;
        self.poisoned = result.is_err();
        result
    }
    /// Prefer `mutate` for owner writes: an owner/capture error permanently
    /// prevents commit, even if a caller catches the error.
    pub async fn mutate<T, F>(&mut self, write: F) -> Result<T>
    where
        F: for<'b> FnOnce(
            &'b Transaction<'a>,
        )
            -> Pin<Box<dyn Future<Output = Result<(T, Vec<Change>)>> + Send + 'b>>,
    {
        if self.poisoned {
            return Err(Error::Internal);
        }
        // A timed-out/cancelled owner future may already have performed SQL.
        // Leave the transaction poisoned until the write and capture finish.
        self.poisoned = true;
        let result = write(&self.tx).await;
        match result {
            Ok((value, changes)) => {
                if changes.is_empty() {
                    self.poisoned = true;
                    return Err(Error::Invalid);
                }
                self.poisoned = false;
                for change in changes {
                    self.record(change).await?;
                }
                Ok(value)
            }
            Err(error) => {
                self.poisoned = true;
                Err(error)
            }
        }
    }
    /// In-process, infallible fan-out only. Durable work belongs in a
    /// transactionally inserted queue/outbox, not in these callbacks.
    pub fn after_commit(&mut self, callback: impl FnOnce() + Send + 'static) {
        self.callbacks.push(Box::new(callback));
    }
    pub async fn rollback(self) -> Result<()> {
        self.tx.rollback().await?;
        Ok(())
    }
    pub async fn commit(self) -> Result<()> {
        if self.poisoned {
            self.tx.rollback().await?;
            return Err(Error::Internal);
        }
        self.capture.action(&self.tx, &self.context).await?;
        self.tx.commit().await?;
        for callback in self.callbacks {
            callback();
        }
        Ok(())
    }
}
