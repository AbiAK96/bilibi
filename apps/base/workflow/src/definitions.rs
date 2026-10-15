//! Immutable compiled adapter catalog. Status configuration is explicitly
//! inserted, never overwritten at boot or on an owner definition change.
use crate::{OwnerFuture, Request, Subject, SubjectAdapter};
use bilibi_database::{Change, Error, Result};
use bilibi_module_registry::{Entry, RegistryError};
use bilibi_tenancy::Scope;
use std::collections::{BTreeMap, BTreeSet};
use tokio_postgres::Transaction;

pub const CONTRIBUTIONS: &str = "base/workflow.definitions";
pub trait Guard: Send + Sync {
    fn check<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        subject: &'a Subject,
        edge: &'a Transition,
        request: &'a Request,
    ) -> OwnerFuture<'a, ()>;
}
pub trait Action: Send + Sync {
    fn execute<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        subject: &'a Subject,
        edge: &'a Transition,
        request: &'a Request,
    ) -> OwnerFuture<'a, Vec<Change>>;
}
#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub code: &'static str,
    pub label: &'static str,
    pub position: i32,
    pub active: bool,
    /// JSON object holding PIC, notifications, comment_tags and prompt data.
    pub settings: &'static str,
}
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub from: &'static str,
    pub to: &'static str,
    pub label: &'static str,
    pub capability: Option<&'static str>,
    pub guard: Option<&'static str>,
    pub action: Option<&'static str>,
    pub position: i32,
    pub active: bool,
    pub sla_seconds: Option<i64>,
    pub metadata: &'static str,
}
/// Resolved, operator-configured edge passed to owner callbacks.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub label: String,
    pub capability: Option<String>,
    pub guard: Option<String>,
    pub action: Option<String>,
    pub position: i32,
    pub sla_seconds: Option<i64>,
    pub metadata: serde_json::Value,
}
#[derive(Clone, Copy, Debug)]
pub struct Flow {
    pub code: &'static str,
    pub label: &'static str,
    pub subject: &'static str,
    pub active: bool,
    pub statuses: &'static [Status],
    pub edges: &'static [Edge],
}
pub enum Contribution {
    Subject {
        key: &'static str,
        aliases: &'static [&'static str],
        adapter: &'static dyn SubjectAdapter,
    },
    Guard {
        key: &'static str,
        aliases: &'static [&'static str],
        adapter: &'static dyn Guard,
    },
    Action {
        key: &'static str,
        aliases: &'static [&'static str],
        adapter: &'static dyn Action,
    },
    Flow(Flow),
}
#[derive(Clone, Copy)]
pub struct OwnedSubject {
    pub owner: &'static str,
    pub key: &'static str,
    pub adapter: &'static dyn SubjectAdapter,
}
#[derive(Default)]
pub struct Registry {
    pub(crate) subjects: BTreeMap<&'static str, OwnedSubject>,
    pub(crate) flows: BTreeMap<&'static str, (&'static str, Flow)>,
    guards: BTreeMap<&'static str, (&'static str, &'static dyn Guard)>,
    actions: BTreeMap<&'static str, (&'static str, &'static dyn Action)>,
    aliases: BTreeMap<(&'static str, &'static str), &'static str>,
}
fn invalid(message: impl Into<String>) -> RegistryError {
    RegistryError::Contribution(message.into())
}
pub fn key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.split('.').all(|part| {
            part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
        })
}
fn object(value: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(value).is_ok_and(|v| v.is_object())
}
impl Registry {
    pub fn from_inventory() -> bilibi_module_registry::Result<Self> {
        let entries = inventory::iter::<bilibi_module_registry::Contribution>
            .into_iter()
            .filter(|entry| entry.consumer == CONTRIBUTIONS)
            .map(|entry| Ok((entry.provenance.module, entry.get::<Contribution>()?)))
            .collect::<bilibi_module_registry::Result<Vec<_>>>()?;
        let vocabulary = bilibi_authz::vocabulary()
            .map_err(|_| invalid("authz vocabulary unavailable for workflow edges"))?;
        Self::build(&entries, Some(vocabulary))
    }
    /// `vocabulary` is `None` only inside the boot-time consumer validator:
    /// discovery runs validators while `bilibi_authz::vocabulary()` initializes,
    /// so calling it there deadlocks. The engine factory proves capabilities.
    pub fn build(
        entries: &[(&'static str, &Contribution)],
        vocabulary: Option<&bilibi_authz::Vocabulary>,
    ) -> bilibi_module_registry::Result<Self> {
        let mut registry = Self::default();
        for &(owner, entry) in entries {
            if owner != "base/workflow"
                && !inventory::iter::<bilibi_module_registry::Module>
                    .into_iter()
                    .any(|m| m.id == owner && m.dependencies.contains(&"base/workflow"))
            {
                return Err(invalid(format!("{owner} must depend on base/workflow")));
            }
            let (kind, k, aliases, duplicate) = match entry {
                Contribution::Subject {
                    key,
                    aliases,
                    adapter,
                } => (
                    "subject",
                    *key,
                    *aliases,
                    registry
                        .subjects
                        .insert(
                            key,
                            OwnedSubject {
                                owner,
                                key,
                                adapter: *adapter,
                            },
                        )
                        .is_some(),
                ),
                Contribution::Guard {
                    key,
                    aliases,
                    adapter,
                } => (
                    "guard",
                    *key,
                    *aliases,
                    registry.guards.insert(key, (owner, *adapter)).is_some(),
                ),
                Contribution::Action {
                    key,
                    aliases,
                    adapter,
                } => (
                    "action",
                    *key,
                    *aliases,
                    registry.actions.insert(key, (owner, *adapter)).is_some(),
                ),
                Contribution::Flow(flow) => (
                    "flow",
                    flow.code,
                    &[][..],
                    registry.flows.insert(flow.code, (owner, *flow)).is_some(),
                ),
            };
            if !key(k) || duplicate {
                return Err(invalid(format!("invalid/duplicate {kind} {k}")));
            }
            for alias in std::iter::once(&k).chain(aliases.iter()) {
                if alias.is_empty()
                    || alias.len() > 255
                    || registry.aliases.insert((kind, alias), k).is_some()
                {
                    return Err(invalid(format!("duplicate/invalid {kind} alias {alias}")));
                }
            }
        }
        let mut bound = BTreeSet::new();
        for &(owner, flow) in registry.flows.values() {
            if flow.label.trim().is_empty()
                || !registry
                    .subjects
                    .get(flow.subject)
                    .is_some_and(|s| s.owner == owner)
                || !bound.insert(flow.subject)
                || flow.statuses.is_empty()
            {
                return Err(invalid("invalid flow ownership or repeated subject"));
            }
            let mut statuses = BTreeSet::new();
            for status in flow.statuses {
                if !key(status.code)
                    || status.label.trim().is_empty()
                    || !object(status.settings)
                    || !statuses.insert(status.code)
                {
                    return Err(invalid("invalid/duplicate status"));
                }
            }
            let mut edges = BTreeSet::new();
            for edge in flow.edges {
                if !statuses.contains(edge.from)
                    || !statuses.contains(edge.to)
                    || !edges.insert((edge.from, edge.to))
                    || edge.label.trim().is_empty()
                    || !object(edge.metadata)
                    || edge.sla_seconds.is_some_and(|s| s < 0)
                    || edge.capability.is_some_and(|k| {
                        !key(k) || vocabulary.is_some_and(|v| !v.capabilities.contains_key(k))
                    })
                {
                    return Err(invalid("invalid edge/status/capability"));
                }
                registry
                    .guard(owner, edge.guard)
                    .map_err(|_| invalid("missing or foreign guard"))?;
                registry
                    .action(owner, edge.action)
                    .map_err(|_| invalid("missing or foreign action"))?;
            }
        }
        Ok(registry)
    }
    pub fn subject(&self, alias: &str) -> Result<(OwnedSubject, Flow)> {
        let key = self
            .aliases
            .get(&("subject", alias))
            .ok_or(Error::Invalid)?;
        let subject = *self.subjects.get(key).ok_or(Error::Invalid)?;
        let flow = self
            .flows
            .values()
            .find(|(_, f)| f.subject == *key)
            .map(|(_, f)| *f)
            .ok_or(Error::Invalid)?;
        Ok((subject, flow))
    }
    pub(crate) fn guard(
        &self,
        owner: &str,
        alias: Option<&str>,
    ) -> Result<Option<&'static dyn Guard>> {
        alias
            .map(|alias| {
                let key = self.aliases.get(&("guard", alias)).ok_or(Error::Invalid)?;
                self.guards
                    .get(key)
                    .filter(|(o, _)| *o == owner)
                    .map(|(_, a)| *a)
                    .ok_or(Error::Invalid)
            })
            .transpose()
    }
    pub(crate) fn action(
        &self,
        owner: &str,
        alias: Option<&str>,
    ) -> Result<Option<&'static dyn Action>> {
        alias
            .map(|alias| {
                let key = self.aliases.get(&("action", alias)).ok_or(Error::Invalid)?;
                self.actions
                    .get(key)
                    .filter(|(o, _)| *o == owner)
                    .map(|(_, a)| *a)
                    .ok_or(Error::Invalid)
            })
            .transpose()
    }
}
fn validate(entries: &[Entry]) -> bilibi_module_registry::Result<()> {
    let entries = entries
        .iter()
        .map(|e| Ok((e.provenance.module, e.get::<Contribution>()?)))
        .collect::<bilibi_module_registry::Result<Vec<_>>>()?;
    Registry::build(&entries, None).map(|_| ())
}
inventory::submit! { bilibi_module_registry::Consumer::new::<Contribution>(CONTRIBUTIONS,"base/workflow",validate) }
