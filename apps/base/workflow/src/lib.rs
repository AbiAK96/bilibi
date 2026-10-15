//! Owner-proven status transitions, versioned process runs, worker leases
//! and the human action gate. All callbacks share the caller's audited
//! transaction; hooks perform database effects only. Durable delivery is W6.
pub mod definitions;
pub mod engine;
pub mod history;
mod intent;
pub mod process;
pub mod runs;
use bilibi_database::{Change, Result};
use bilibi_tenancy::Scope;
pub use definitions::{
    Action, Contribution, Edge, Flow, Guard, HumanAction, HumanActionHandler, HumanOutcome,
    Registry, Status, Transition, CONTRIBUTIONS,
};
pub use engine::{Engine, Request};
pub use process::{
    Dependency, DependencyMode, Process, ProcessAdapter, ProcessDefinition, RunFacts, Step,
    StepDefinition,
};
pub use runs::{
    ActionRequest, ActionResult, AvailableAction, AvailableActions, Claim, ClaimOptions, Claimed,
    Completion, Event, EventPage, Run, RunCommand, RunView, StartOptions, WorkCommand, WorkItem,
    WorkView,
};
use serde_json::Value;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio_postgres::Transaction;

pub type OwnerFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
#[derive(Clone, Debug)]
pub struct Subject {
    pub id: i64,
    pub tenant: i64,
    pub status: String,
    pub version: i64,
    pub facts: Value,
}
#[derive(Clone, Copy, Debug)]
pub enum Load {
    Read,
    Lock,
}
#[derive(Clone, Copy, Debug)]
pub enum Operation {
    Read,
    Transition,
    RecordInitial,
    RecordComment,
    /// Installed owner binds an existing subject and its history, under its
    /// own explicit adoption policy; never a tenant guessed from history.
    Adopt,
    /// Process run operations, asked of both the subject and process adapters.
    /// `ReadRun` never grants execution.
    StartRun,
    ReadRun,
    ReconcileRun,
    CompleteWork,
    SignalRun,
    PauseRun,
    ResumeRun,
    SupersedeRun,
    /// Worker lease operations; a leaseholder's completion asks `CompleteWork`.
    ClaimWork,
    HeartbeatWork,
    FailWork,
    BlockClaimedWork,
    /// Owner or operator finishing unfinished work with a reason.
    WaiveWork,
    BlockWork,
    /// A signed-in person executing a human action on the locked subject;
    /// listing available actions asks `Read`. A work-bound action then asks
    /// both adapters `CompleteWork` for its run.
    ExecuteAction,
}
/// The owner facade accepts the kernel transaction, scopes every query, locks
/// and rereads on `Load::Lock`, and enforces live record policy even on edges
/// without a capability. Never open/commit another transaction in a callback.
pub trait SubjectAdapter: Send + Sync {
    fn load<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        id: i64,
        mode: Load,
    ) -> OwnerFuture<'a, Subject>;
    fn authorize<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        subject: &'a Subject,
        operation: Operation,
    ) -> OwnerFuture<'a, ()>;
    /// Return every owner mutation for capture. Compare against the locked
    /// status/version and increment the version; no effects outside PostgreSQL.
    fn persist<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        subject: &'a Subject,
        to: &'a str,
        request: &'a Request,
    ) -> OwnerFuture<'a, Vec<Change>>;
}

inventory::submit! { bilibi_module_registry::Module {
    id:"base/workflow",layer:bilibi_module_registry::Layer::Base,
    dependencies:&["base/module_registry","base/database","base/audit","base/authz","base/tenancy","base/queue"],
    migrations:&[("workflow-001-status-kernel",include_str!("../migrations/001-status.sql")),
        ("workflow-002-process-graphs",include_str!("../migrations/002-process.sql")),
        ("workflow-003-human-actions",include_str!("../migrations/003-human-actions.sql"))],
    services:&[bilibi_module_registry::Service {id:"base/workflow.engine",factory:|services| {
        Ok(Arc::new(Engine::new(
            services.get::<bilibi_database::Db>("base/database.pool")?.as_ref().clone(),
            services.get("base/database.mutations")?, services.get("base/tenancy.authentication")?,
            Registry::from_inventory()?,
        )))
    }}],
} }

inventory::submit! { bilibi_module_registry::Contribution::new(
bilibi_authz::VOCABULARY,"workflow.vocabulary",bilibi_module_registry::Provenance {module:"base/workflow",source:file!(),line:line!()},
&bilibi_authz::VocabularyContribution {domains:&["workflow"],verbs:&[],
    capabilities:&[bilibi_authz::CapabilityDefinition::platform("admin.workflow.definition.manage")],
    roles:&[],system_principals:&[]}) }
