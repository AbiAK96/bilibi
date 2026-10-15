//! Bounded tenant reads of the action ledger for the Audit screens.
//!
//! These functions answer "what is recorded", never "who may ask": the caller
//! (`base/audit_views`) has already proven the capability, because this crate
//! sits below Authz and cannot ask it. Everything is scoped to the sealed
//! scope's tenant in SQL; free text reaches SQL only as a bound, escaped pattern
//! and every sort key is a fixed column name.
use bilibi_database::{Error, Result};
use bilibi_tenancy::Scope;
use serde_json::Value;
use std::time::SystemTime;
use tokio_postgres::{types::ToSql, GenericClient, Transaction};

/// Free-text search is bounded before it reaches SQL.
pub const MAX_SEARCH: usize = 200;
/// The largest operational-list page (25/50/100/300).
pub const MAX_PAGE_SIZE: u64 = 300;
/// Event families the Actions screen filters by, with the ledger's own names.
pub const EVENT_FAMILIES: [&str; 6] = ["http", "auth", "console", "database", "queue", "domain"];

/// Sortable action columns. Every order is followed by `id` in the same
/// direction, so equal values page stably.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionSort {
    OccurredAt,
    ActorType,
    Event,
    Url,
    TraceId,
}
impl ActionSort {
    pub fn parse(key: &str) -> Option<Self> {
        Some(match key {
            "occurred_at" => Self::OccurredAt,
            "actor_type" => Self::ActorType,
            "event" => Self::Event,
            "url" => Self::Url,
            "trace_id" => Self::TraceId,
            _ => return None,
        })
    }
    fn column(self) -> &'static str {
        match self {
            Self::OccurredAt => "a.occurred_at",
            Self::ActorType => "a.actor_type",
            Self::Event => "a.event",
            Self::Url => "a.url",
            Self::TraceId => "a.trace_id",
        }
    }
}

/// What the Actions screen asks for. Empty strings mean "any".
#[derive(Clone, Debug)]
pub struct ActionQuery {
    pub search: String,
    pub actor_type: String,
    /// One of [`EVENT_FAMILIES`], or empty.
    pub event_family: String,
    /// `failure`, `retained`, or empty.
    pub result: String,
    /// Transport noise (Livewire-style updates, event streams, media streams)
    /// is hidden unless asked for; failures always appear.
    pub diagnostics: bool,
    pub sort: ActionSort,
    pub descending: bool,
    /// One-based.
    pub page: u64,
    pub page_size: u64,
}
impl Default for ActionQuery {
    fn default() -> Self {
        Self {
            search: String::new(),
            actor_type: String::new(),
            event_family: String::new(),
            result: String::new(),
            diagnostics: false,
            sort: ActionSort::OccurredAt,
            descending: true,
            page: 1,
            page_size: 25,
        }
    }
}

/// One recorded action as the list shows it. `is_retained` is the bookmark
/// when one exists, otherwise the bit recorded with the action.
#[derive(Clone, Debug)]
pub struct ActionEntry {
    pub id: i64,
    pub actor_type: String,
    pub actor_id: i64,
    pub actor_role: Option<String>,
    pub impersonator_id: Option<i64>,
    pub system_principal: Option<String>,
    pub url: Option<String>,
    pub event: String,
    pub payload: Value,
    pub trace_id: Option<String>,
    pub is_retained: bool,
    pub occurred_at: SystemTime,
}
pub struct ActionPage {
    pub entries: Vec<ActionEntry>,
    pub total_entries: u64,
}

/// A mutation as the dashboard's recent-activity feed shows it: what happened
/// to which kind of record, and when. No values, so nothing needs withholding.
#[derive(Clone, Debug)]
pub struct RecentMutation {
    pub id: i64,
    pub event: String,
    pub auditable_type: String,
    pub occurred_at: SystemTime,
}

/// Search text is literal: `%`, `_` and `\` match themselves.
fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// A payload field as an integer, 0 when absent or not a number: recorded
/// payloads are free-form, and a stray word must not fail the whole list.
const STATUS: &str =
    "(CASE WHEN a.payload->>'status' ~ '^[0-9]{1,9}$' THEN (a.payload->>'status')::int ELSE 0 END)";
const EXIT_CODE: &str = "(CASE WHEN a.payload->>'exit_code' ~ '^-?[0-9]{1,9}$' THEN (a.payload->>'exit_code')::int ELSE 0 END)";

/// One page of the tenant's actions. An out-of-range page returns no entries
/// with the real total, so the caller can land on the last page.
pub async fn list_actions(
    client: &impl GenericClient,
    scope: &Scope,
    query: &ActionQuery,
) -> Result<ActionPage> {
    let search = query.search.trim();
    if search.len() > MAX_SEARCH
        || query.page == 0
        || query.page_size == 0
        || query.page_size > MAX_PAGE_SIZE
    {
        return Err(Error::Invalid);
    }
    let tenant = scope.tenant();
    let pattern = format!("%{}%", escape_like(search));
    let mut filters = vec!["a.tenant_id=$1".to_owned()];
    let mut params: Vec<&(dyn ToSql + Sync)> = vec![&tenant];
    if !search.is_empty() {
        params.push(&pattern);
        let n = params.len();
        filters.push(format!(
            "(a.event ILIKE ${n} OR coalesce(a.actor_role,'') ILIKE ${n} OR coalesce(a.url,'') ILIKE ${n} OR coalesce(a.trace_id,'') ILIKE ${n} OR a.actor_type ILIKE ${n} OR a.payload::text ILIKE ${n})"
        ));
    }
    if !query.actor_type.is_empty() {
        params.push(&query.actor_type);
        filters.push(format!("a.actor_type=${}", params.len()));
    }
    // Fixed fragments only: a family outside the list adds no filter, as in Bilimbi.
    match query.event_family.as_str() {
        "http" => filters.push("a.event='http.request'".into()),
        "auth" => filters.push("a.event LIKE 'auth.%'".into()),
        "console" => filters.push("a.event='console.command'".into()),
        "database" => filters.push("a.event LIKE 'database\\_query.%'".into()),
        "queue" => filters.push("a.event LIKE 'queue.job.%'".into()),
        "domain" => filters.push("a.event LIKE 'domain.%'".into()),
        _ => {}
    }
    match query.result.as_str() {
        "retained" => filters.push("coalesce(m.is_retained,a.is_retained)".into()),
        "failure" => filters.push(format!(
            "(a.event IN ('auth.login.failed','queue.job.failed') \
              OR (a.event='http.request' AND {STATUS}>=400) \
              OR (a.event='console.command' AND {EXIT_CODE}<>0) \
              OR a.event IN ('database_query.refused','database_query.failed') \
              OR a.payload::text ILIKE '%failed%')"
        )),
        _ => {}
    }
    if !query.diagnostics {
        filters.push(format!(
            "(a.event<>'http.request' OR {STATUS}>=400 \
              OR (coalesce(a.url,'') NOT ILIKE '%/livewire%' \
                  AND coalesce(a.url,'') NOT ILIKE '%/api/ai/chat/turns/%' \
                  AND coalesce(a.url,'') NOT ILIKE '%/media/assets/%' \
                  AND a.payload::text NOT ILIKE '%default-livewire.update%'))"
        ));
    }
    let filter = filters.join(" AND ");
    let from =
        "FROM base_audit_actions a LEFT JOIN base_audit_action_metadata m ON m.action_id=a.id";
    let total: i64 = client
        .query_one(&format!("SELECT count(*) {from} WHERE {filter}"), &params)
        .await?
        .get(0);
    let direction = if query.descending { "DESC" } else { "ASC" };
    let limit = i64::try_from(query.page_size).map_err(|_| Error::Invalid)?;
    let offset = i64::try_from((query.page - 1).saturating_mul(query.page_size))
        .map_err(|_| Error::Invalid)?;
    params.push(&limit);
    params.push(&offset);
    let rows = client
        .query(
            &format!(
                "SELECT a.id,a.actor_type,a.actor_id,a.actor_role,a.impersonator_id,a.system_principal,a.url,a.event,\
                        coalesce(a.payload,'{{}}'::jsonb),a.trace_id,coalesce(m.is_retained,a.is_retained),a.occurred_at \
                 {from} WHERE {filter} ORDER BY {column} {direction}, a.id {direction} LIMIT ${l} OFFSET ${o}",
                column = query.sort.column(),
                l = params.len() - 1,
                o = params.len(),
            ),
            &params,
        )
        .await?;
    Ok(ActionPage {
        entries: rows
            .iter()
            .map(|row| ActionEntry {
                id: row.get(0),
                actor_type: row.get(1),
                actor_id: row.get(2),
                actor_role: row.get(3),
                impersonator_id: row.get(4),
                system_principal: row.get(5),
                url: row.get(6),
                event: row.get(7),
                payload: row.get(8),
                trace_id: row.get(9),
                is_retained: row.get(10),
                occurred_at: row.get(11),
            })
            .collect(),
        total_entries: u64::try_from(total).unwrap_or(0),
    })
}

/// Flips the retention bookmark of one action in this tenant, under a row
/// lock, and returns `(before, after)`; `None` when the tenant has no such
/// action. The recorded `is_retained` bit is evidence and never changes; the
/// bookmark in `base_audit_action_metadata` overrides it. The caller must hold
/// the manage capability and own the transaction.
pub async fn toggle_retained(
    tx: &Transaction<'_>,
    scope: &Scope,
    action_id: i64,
) -> Result<Option<(bool, bool)>> {
    let Some(row) = tx
        .query_opt(
            "SELECT coalesce(m.is_retained,a.is_retained) FROM base_audit_actions a \
             LEFT JOIN base_audit_action_metadata m ON m.action_id=a.id \
             WHERE a.id=$1 AND a.tenant_id=$2 FOR UPDATE OF a",
            &[&action_id, &scope.tenant()],
        )
        .await?
    else {
        return Ok(None);
    };
    let before: bool = row.get(0);
    tx.execute(
        "INSERT INTO base_audit_action_metadata(action_id,is_retained) VALUES($1,$2) \
         ON CONFLICT (action_id) DO UPDATE SET is_retained=EXCLUDED.is_retained",
        &[&action_id, &!before],
    )
    .await?;
    Ok(Some((before, !before)))
}

/// The tenant's latest mutations, newest first.
pub async fn recent_mutations(
    client: &impl GenericClient,
    scope: &Scope,
    limit: i64,
) -> Result<Vec<RecentMutation>> {
    if !(1..=200).contains(&limit) {
        return Err(Error::Invalid);
    }
    Ok(client
        .query(
            "SELECT id,event,auditable_type,occurred_at FROM base_audit_mutations \
             WHERE tenant_id=$1 ORDER BY occurred_at DESC,id DESC LIMIT $2",
            &[&scope.tenant(), &limit],
        )
        .await?
        .iter()
        .map(|row| RecentMutation {
            id: row.get(0),
            event: row.get(1),
            auditable_type: row.get(2),
            occurred_at: row.get(3),
        })
        .collect())
}
