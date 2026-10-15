//! Immutable contributed recurrences, fingerprint review and suppression.
//! Occurrence claiming, enqueue and history build on this (Q4); the operator
//! board is Q5. Nothing here runs work.
pub mod cron;
pub mod definition;
pub mod recurrence;
pub use definition::{Definition, Misfire, Overlap, DEFINITIONS, RESERVED_ARGUMENT};

use bilibi_authz::{
    authorize_key, authorize_operator, CapabilityDefinition, RoleDefinition,
    VocabularyContribution, VOCABULARY,
};
use bilibi_database::{Change, Error, MutationBoundary, Result};
use bilibi_module_registry::{Catalog, Contribution, Module, Provenance, RegistryError, Service};
use bilibi_queue::Queue;
use bilibi_tenancy::Scope;
use jiff::{tz::TimeZone, Timestamp};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};
use tokio_postgres::{Client, GenericClient, Transaction};

pub const VIEW: &str = "admin.system.schedule.view";
pub const EXECUTE: &str = "admin.system.schedule.execute";
pub const MANAGE: &str = "admin.system.schedule.manage";

/// A validated definition with its owner, parsed recurrence and fingerprint.
pub struct Registered {
    key: &'static str,
    owner: &'static str,
    definition: &'static Definition,
    cron: cron::Cron,
    zone: TimeZone,
    args: Value,
    fingerprint: String,
}
impl Registered {
    pub fn key(&self) -> &'static str {
        self.key
    }
    /// The contributing module.
    pub fn owner(&self) -> &'static str {
        self.owner
    }
    pub fn definition(&self) -> &'static Definition {
        self.definition
    }
    pub fn zone(&self) -> &TimeZone {
        &self.zone
    }
    /// Worker-validated arguments exactly as Q4 will enqueue them.
    pub fn args(&self) -> &Value {
        &self.args
    }
    /// Hex SHA-256 of every material field; review binds to this value.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    /// Earliest occurrence at or after `now` (`now` is the caller's clock).
    pub fn next(&self, now: Timestamp) -> std::result::Result<Timestamp, recurrence::Error> {
        recurrence::next(&self.cron, &self.zone, now)
    }
    /// Latest occurrence at or before `now`.
    pub fn previous(&self, now: Timestamp) -> std::result::Result<Timestamp, recurrence::Error> {
        recurrence::previous(&self.cron, &self.zone, now)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewState {
    /// Never reviewed, or reviewed at a fingerprint the definition no longer has.
    Unreviewed,
    Enabled,
    Disabled,
}
/// Whether work may be claimed now. Anything but `Eligible` must not run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Eligibility {
    Eligible,
    Unreviewed,
    Disabled,
    Suppressed,
}

/// Operator-facing facts; no worker arguments.
#[derive(Clone, Debug)]
pub struct Task {
    pub key: &'static str,
    pub name: &'static str,
    pub owner: &'static str,
    pub owner_route: Option<&'static str>,
    pub task_name: &'static str,
    pub expression: &'static str,
    pub timezone: &'static str,
    pub overlap: Overlap,
    pub misfire: Misfire,
    pub fingerprint: String,
    pub review: ReviewState,
    pub suppressed: bool,
    pub next_due_at: Option<Timestamp>,
}

pub struct Schedule {
    definitions: BTreeMap<&'static str, Registered>,
    boundary: Arc<MutationBoundary>,
}
impl Schedule {
    /// Validates every contribution against the Queue's worker catalog: the
    /// worker must exist, belong to the contributing module, and accept the args.
    pub fn from_catalog(
        catalog: &Catalog,
        queue: &Queue,
        boundary: Arc<MutationBoundary>,
    ) -> bilibi_module_registry::Result<Self> {
        let mut definitions = BTreeMap::new();
        for entry in catalog.entries::<Definition>(DEFINITIONS)? {
            let fail = |reason: &str| {
                RegistryError::Contribution(format!(
                    "schedule definition {} from {} at {}:{}: {reason}",
                    entry.id,
                    entry.provenance.module,
                    entry.provenance.source,
                    entry.provenance.line
                ))
            };
            let definition = entry.get::<Definition>()?;
            let cron = definition::check(entry.id, definition).map_err(|reason| fail(&reason))?;
            let zone = TimeZone::get(definition.timezone).map_err(|_| fail("unknown timezone"))?;
            let worker = queue
                .workers()
                .get(definition.worker)
                .map_err(|_| fail("worker is not a registered Queue worker"))?;
            if worker.module != entry.provenance.module {
                return Err(fail("worker belongs to another module"));
            }
            let args = (definition.args)();
            let normalized = (worker.definition.validate)(args.clone())
                .map_err(|_| fail("worker refuses the arguments"))?;
            bilibi_queue::arguments::validate(&normalized)
                .map_err(|_| fail("normalized arguments exceed the transport bounds"))?;
            definitions.insert(
                entry.id,
                Registered {
                    key: entry.id,
                    owner: entry.provenance.module,
                    definition,
                    cron,
                    zone,
                    fingerprint: definition::fingerprint(
                        entry.id,
                        entry.provenance.module,
                        definition,
                        &args,
                    ),
                    args: normalized,
                },
            );
        }
        Ok(Self {
            definitions,
            boundary,
        })
    }
    pub fn definitions(&self) -> impl Iterator<Item = &Registered> {
        self.definitions.values()
    }
    pub fn definition(&self, key: &str) -> Option<&Registered> {
        self.definitions.get(key)
    }

    /// Operator view. `now` is the caller's clock, so a replayed instant yields
    /// the same `next_due_at` it would have produced then.
    pub async fn tasks(
        &self,
        client: &(impl GenericClient + Sync),
        scope: &Scope,
        now: Timestamp,
    ) -> Result<Vec<Task>> {
        authorize_key(client, scope, VIEW).await?;
        let keys: Vec<&str> = self.definitions.keys().copied().collect();
        let reviews: BTreeMap<String, (String, bool)> = client
            .query(
                "SELECT key,fingerprint,enabled FROM base_schedule_definition_reviews WHERE key=ANY($1)",
                &[&keys],
            )
            .await?
            .into_iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    (row.get::<_, String>(1), row.get::<_, bool>(2)),
                )
            })
            .collect();
        let suppressed: Vec<String> = client
            .query(
                "SELECT key FROM base_schedule_suppressions WHERE key=ANY($1)",
                &[&keys],
            )
            .await?
            .into_iter()
            .map(|row| row.get(0))
            .collect();
        Ok(self
            .definitions
            .values()
            .map(|item| Task {
                key: item.key,
                name: item.definition.name,
                owner: item.owner,
                owner_route: item.definition.owner_route,
                task_name: item.definition.task_name,
                expression: item.definition.expression,
                timezone: item.definition.timezone,
                overlap: item.definition.overlap,
                misfire: item.definition.misfire,
                fingerprint: item.fingerprint.clone(),
                review: review_state(item, reviews.get(item.key)),
                suppressed: suppressed.iter().any(|key| key == item.key),
                next_due_at: item.next(now).ok(),
            })
            .collect())
    }

    /// Trusted claim-time check for Q4 and queued-job start. Call it inside the
    /// transaction that holds [`lock`] so a concurrent review or pause cannot
    /// slip between the read and the claim. A database error is an `Err`, never
    /// `Eligible`: uncertainty fails closed.
    pub async fn eligibility(&self, client: &impl GenericClient, key: &str) -> Result<Eligibility> {
        let item = self.definitions.get(key).ok_or(Error::NotFound)?;
        let review = client
            .query_opt(
                "SELECT fingerprint,enabled FROM base_schedule_definition_reviews WHERE key=$1",
                &[&key],
            )
            .await?
            .map(|row| (row.get::<_, String>(0), row.get::<_, bool>(1)));
        let paused = client
            .query_opt(
                "SELECT 1 FROM base_schedule_suppressions WHERE key=$1",
                &[&key],
            )
            .await?
            .is_some();
        Ok(match review_state(item, review.as_ref()) {
            ReviewState::Unreviewed => Eligibility::Unreviewed,
            ReviewState::Disabled => Eligibility::Disabled,
            ReviewState::Enabled if paused => Eligibility::Suppressed,
            ReviewState::Enabled => Eligibility::Eligible,
        })
    }

    /// Records the decision for the definition's *current* fingerprint. A later
    /// change to any material field makes it unreviewed again.
    pub async fn review(
        &self,
        client: &mut Client,
        scope: &Scope,
        key: &str,
        enabled: bool,
    ) -> Result<()> {
        authorize_operator(&*client, scope, MANAGE).await?;
        let item = self.definitions.get(key).ok_or(Error::NotFound)?;
        let fingerprint = item.fingerprint.clone();
        let action = if enabled {
            "schedule.task.enabled"
        } else {
            "schedule.task.disabled"
        };
        let mut tx = self
            .boundary
            .begin(client, scope.mutation_context(action))
            .await?;
        lock(tx.sql(), key).await?;
        let key = key.to_owned();
        tx.mutate(move |sql| {
            Box::pin(async move {
                let old = sql
                    .query_opt(
                        "SELECT fingerprint,enabled FROM base_schedule_definition_reviews WHERE key=$1",
                        &[&key],
                    )
                    .await?
                    .map(|row| json!({"fingerprint": row.get::<_, String>(0), "enabled": row.get::<_, bool>(1)}))
                    .unwrap_or(Value::Null);
                let id: i64 = sql
                    .query_one(
                        "INSERT INTO base_schedule_definition_reviews(key,fingerprint,enabled) VALUES($1,$2,$3) \
                         ON CONFLICT (key) DO UPDATE SET fingerprint=EXCLUDED.fingerprint,enabled=EXCLUDED.enabled,reviewed_at=now() RETURNING id",
                        &[&key, &fingerprint, &enabled],
                    )
                    .await?
                    .get(0);
                Ok((
                    (),
                    vec![Change {
                        kind: "base/schedule.review".into(),
                        id,
                        old,
                        new: json!({"key": key, "fingerprint": fingerprint, "enabled": enabled}),
                    }],
                ))
            })
        })
        .await?;
        tx.commit().await
    }

    /// Pauses a definition (a row means paused). Returns whether anything changed.
    pub async fn suppress(&self, client: &mut Client, scope: &Scope, key: &str) -> Result<bool> {
        self.set_suppressed(client, scope, key, true).await
    }
    /// Resumes a definition (deleting the row means resumed).
    pub async fn resume(&self, client: &mut Client, scope: &Scope, key: &str) -> Result<bool> {
        self.set_suppressed(client, scope, key, false).await
    }
    async fn set_suppressed(
        &self,
        client: &mut Client,
        scope: &Scope,
        key: &str,
        suppressed: bool,
    ) -> Result<bool> {
        authorize_operator(&*client, scope, MANAGE).await?;
        self.definitions.get(key).ok_or(Error::NotFound)?;
        let action = if suppressed {
            "schedule.task.paused"
        } else {
            "schedule.task.resumed"
        };
        let mut tx = self
            .boundary
            .begin(client, scope.mutation_context(action))
            .await?;
        lock(tx.sql(), key).await?;
        let present = tx
            .sql()
            .query_opt(
                "SELECT 1 FROM base_schedule_suppressions WHERE key=$1",
                &[&key],
            )
            .await?
            .is_some();
        // A repeat command changes nothing, so it records nothing.
        if present == suppressed {
            tx.rollback().await?;
            return Ok(false);
        }
        let key = key.to_owned();
        tx.mutate(move |sql| {
            Box::pin(async move {
                let id: i64 = if suppressed {
                    sql.query_one(
                        "INSERT INTO base_schedule_suppressions(key) VALUES($1) RETURNING id",
                        &[&key],
                    )
                    .await?
                    .get(0)
                } else {
                    sql.query_one(
                        "DELETE FROM base_schedule_suppressions WHERE key=$1 RETURNING id",
                        &[&key],
                    )
                    .await?
                    .get(0)
                };
                Ok((
                    (),
                    vec![Change {
                        kind: "base/schedule.suppression".into(),
                        id,
                        old: json!({"key": key, "suppressed": !suppressed}),
                        new: json!({"key": key, "suppressed": suppressed}),
                    }],
                ))
            })
        })
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

/// Serialises review, suppression and (Q4) claims for one key until the
/// surrounding transaction ends.
pub async fn lock(tx: &Transaction<'_>, key: &str) -> Result<()> {
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        &[&format!("scheduler:{key}")],
    )
    .await?;
    Ok(())
}

fn review_state(item: &Registered, review: Option<&(String, bool)>) -> ReviewState {
    match review {
        Some((fingerprint, enabled)) if *fingerprint == item.fingerprint => {
            if *enabled {
                ReviewState::Enabled
            } else {
                ReviewState::Disabled
            }
        }
        _ => ReviewState::Unreviewed,
    }
}

inventory::submit! { Contribution::new(VOCABULARY, "base/schedule", Provenance { module: "base/schedule", source: file!(), line: line!() }, &VocabularyContribution {
    domains: &[],
    verbs: &["execute"],
    // Installation-global: the persistent operator identity is required too.
    capabilities: &[CapabilityDefinition::platform(VIEW), CapabilityDefinition::platform(EXECUTE), CapabilityDefinition::platform(MANAGE)],
    roles: &[RoleDefinition { code: "system_viewer", name: "System Viewer", description: "Contributed system role; reconciled explicitly", grant_all: false, capabilities: &[VIEW] }],
    system_principals: &[],
}) }
inventory::submit! {
    Module { id: "base/schedule", layer: bilibi_module_registry::Layer::Base,
        dependencies: &["base/module_registry", "base/database", "base/audit", "base/tenancy", "base/authz", "base/queue"],
        migrations: &[("base-schedule-001", include_str!("../migrations/001-schedule.sql"))],
        services: &[Service { id: "base/schedule.store", factory: |services| {
            let catalog = Catalog::discover(&bilibi_module_registry::discover()?)?;
            let queue = services.get::<Queue>("base/queue.store")?;
            let boundary = services.get::<MutationBoundary>("base/database.mutations")?;
            Ok(Arc::new(Schedule::from_catalog(&catalog, &queue, boundary)?))
        } }],
    }
}
