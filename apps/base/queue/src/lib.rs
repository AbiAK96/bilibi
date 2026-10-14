//! Durable queue insertion joins the owner's audited transaction. Q2 owns claims
//! and execution; no callback, spawned task or process-local list is a job.
pub mod arguments;
mod registry;
pub use registry::{RegisteredWorker, Worker, Workers, WORKERS};

use bilibi_database::{Change, Error, MutationTransaction, Result};
use bilibi_module_registry::{Catalog, Module, Service};
use bilibi_tenancy::Scope;
use serde_json::{json, Value};
use std::sync::Arc;

/// Stable reference; transport arguments and actor metadata never escape here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobRef {
    id: i64,
}
impl JobRef {
    pub fn id(self) -> i64 {
        self.id
    }
}
pub struct Queue {
    workers: Workers,
}
impl Queue {
    pub fn new(workers: Workers) -> Self {
        Self { workers }
    }
    pub fn workers(&self) -> &Workers {
        &self.workers
    }

    /// Captures identity from a sealed scope, never from args or caller metadata.
    /// Scope and audit context must agree. Caller retains commit/rollback control;
    /// `JobRef` is provisional until that outer transaction commits.
    /// This grants no capability: the owner must authorize its command and Q2
    /// must rehydrate current actor eligibility and reauthorize every execution.
    pub async fn enqueue_for(
        &self,
        tx: &mut MutationTransaction<'_>,
        scope: &Scope,
        worker_id: &str,
        args: Value,
    ) -> Result<JobRef> {
        if tx.context().tenant != scope.tenant()
            || tx.context().actor.kind != "user"
            || tx.context().actor.id != scope.user().to_string()
            || tx.context().impersonator.is_some()
        {
            return Err(Error::Forbidden);
        }
        let worker = self.workers.get(worker_id)?;
        arguments::validate(&args)?;
        let args = (worker.definition.validate)(args)?;
        arguments::validate(&args)?;
        let tenant = scope.tenant();
        let actor = scope.user();
        let person = scope.actor().person().map(|id| id.get());
        let organisation = scope.actor().organisation().map(|id| id.get());
        let affiliation = scope.actor().affiliation().map(|id| id.get());
        let trace = tx.context().trace_id.clone();
        tx.mutate(move |sql|Box::pin(async move {
            let row = sql.query_one("INSERT INTO base_queue_jobs(tenant_id,worker_id,worker_module,args,actor_kind,actor_id,person_id,organisation_id,affiliation_id,trace_id,max_attempts) VALUES($1,$2,$3,$4,'user',$5,$6,$7,$8,$9,$10) RETURNING id", &[&tenant,&worker.id,&worker.module,&args,&actor,&person,&organisation,&affiliation,&trace,&worker.definition.max_attempts]).await?;
            let id = row.get("id");
            // No args, names, email or credential material enters audit evidence.
            let change = Change {kind:"base/queue.job".into(),id,old:Value::Null,new:json!({"worker":worker.id,"state":"available","actor_kind":"user","actor_id":actor,"max_attempts":worker.definition.max_attempts})};
            Ok((JobRef{id},vec![change]))
        })).await
    }
}
inventory::submit! {
    Module {id:"base/queue",layer:bilibi_module_registry::Layer::Base,
        dependencies:&["base/module_registry","base/database","base/audit","base/tenancy"],
        migrations:&[("base-queue-001",include_str!("../migrations/001-queue.sql"))],
        services:&[Service {id:"base/queue.store",factory:|_| {
            let catalog = Catalog::discover(&bilibi_module_registry::discover()?)?;
            Ok(Arc::new(Queue::new(Workers::from_catalog(&catalog)?)))
        }}],
    }
}
