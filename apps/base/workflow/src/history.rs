use crate::{
    definitions::{Flow, OwnedSubject},
    Engine, Load, Operation, Request, Subject,
};
use bilibi_database::{Change, Context, Error, Result};
use bilibi_tenancy::Scope;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_postgres::{Row, Transaction};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fact {
    pub id: i64,
    pub from_status: String,
    pub status: String,
    pub kind: String,
    pub version: i64,
    pub actor_type: String,
    pub actor_id: String,
    pub impersonator: Option<Value>,
    pub comment: Option<String>,
    pub comment_tag: Option<String>,
    pub assignees: Value,
    pub attachments: Value,
    pub metadata: Value,
    pub tat: Option<i64>,
    pub transitioned_at: String,
}
fn fact(row: Row) -> Fact {
    Fact {
        id: row.get(0),
        from_status: row.get(1),
        status: row.get(2),
        kind: row.get(3),
        version: row.get(4),
        actor_type: row.get(5),
        actor_id: row.get(6),
        impersonator: row.get(7),
        comment: row.get(8),
        comment_tag: row.get(9),
        assignees: row.get(10),
        attachments: row.get(11),
        metadata: row.get(12),
        tat: row.get(13),
        transitioned_at: row.get(14),
    }
}
pub(crate) async fn bind(
    sql: &Transaction<'_>,
    scope: &Scope,
    owner: OwnedSubject,
    flow: Flow,
    id: i64,
) -> Result<i64> {
    sql.execute("INSERT INTO base_workflow_bindings(tenant_id,flow,subject_type,subject_id,owner) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING",&[&scope.tenant(),&flow.code,&owner.key,&id,&owner.owner]).await?;
    // Global uniqueness is checked after locking the proven owner. A tenant
    // scoped miss must never rebind an existing subject to a different tenant.
    let row=sql.query_one("SELECT id,tenant_id,subject_type,owner FROM base_workflow_bindings WHERE flow=$1 AND subject_id=$2 FOR UPDATE",&[&flow.code,&id]).await?;
    if row.get::<_, i64>(1) != scope.tenant()
        || row.get::<_, String>(2) != owner.key
        || row.get::<_, String>(3) != owner.owner
    {
        return Err(Error::Conflict);
    }
    Ok(row.get(0))
}
pub(crate) async fn append(
    sql: &Transaction<'_>,
    context: &Context,
    binding: i64,
    (subject, next): (&Subject, &Subject),
    operation: Operation,
    request: &Request,
) -> Result<(Fact, Change)> {
    let (status, version) = (next.status.as_str(), next.version);
    let kind = match operation {
        Operation::Transition => "transition",
        Operation::RecordInitial => "record_initial",
        Operation::RecordComment => "record_comment",
        _ => return Err(Error::Invalid),
    };
    let impersonator = context
        .impersonator
        .as_ref()
        .map(|p| json!({"kind":p.kind,"id":p.id}));
    let mut metadata = request.metadata.clone();
    metadata["_workflow"] =
        json!({"from_status":subject.status,"kind":kind,"trace_id":context.trace_id});
    let row=sql.query_one("INSERT INTO base_workflow_status_history(binding_id,from_status,status,kind,subject_version,actor_type,actor_id,impersonator,comment,comment_tag,assignees,attachments,metadata,tat) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,CASE WHEN $4='transition' THEN (SELECT greatest(0,floor(extract(epoch FROM clock_timestamp()-transitioned_at)))::bigint FROM base_workflow_status_history WHERE binding_id=$1 AND kind<>'record_comment' ORDER BY id DESC LIMIT 1) END) RETURNING id,from_status,status,kind,subject_version,actor_type,actor_id,impersonator,comment,comment_tag,assignees,attachments,metadata,tat,transitioned_at::text",&[&binding,&subject.status,&status,&kind,&version,&context.actor.kind,&context.actor.id,&impersonator,&request.comment,&request.comment_tag,&request.assignees,&request.attachments,&metadata]).await?;
    let fact = fact(row);
    // Audit the fact identity/status only, not arbitrary input or owner secrets.
    let change = Change {
        kind: "workflow.history".into(),
        id: fact.id,
        old: json!({}),
        new: json!({"binding":binding,"from":subject.status,"status":status,"kind":kind,"version":version}),
    };
    Ok((fact, change))
}
#[derive(Clone, Debug)]
pub struct Page {
    pub entries: Vec<Fact>,
    pub next_cursor: Option<i64>,
}
impl Engine {
    /// Stable append order, bounded keyset page, with owner authorization on
    /// every read. Tenant identity is never inferred from historical facts.
    /// Each fact's `transitioned_at` is RFC 3339 UTC, so a page renders it
    /// through its display context without a session time zone.
    pub async fn history(
        &self,
        scope: &Scope,
        kind: &str,
        id: i64,
        after: Option<i64>,
        limit: i64,
    ) -> Result<Page> {
        if !(1..=500).contains(&limit) || after.is_some_and(|v| v <= 0) {
            return Err(Error::Invalid);
        }
        let mut client = self.db.get().await?;
        let sql = client.transaction().await?;
        let scope = self.live(&sql, scope).await?;
        let (owner, flow) = self.registry.subject(kind)?;
        self.load(&sql, &scope, owner, id, Load::Read, Operation::Read)
            .await?;
        let rows=sql.query("SELECT h.id,h.from_status,h.status,h.kind,h.subject_version,h.actor_type,h.actor_id,h.impersonator,h.comment,h.comment_tag,h.assignees,h.attachments,h.metadata,h.tat,to_char(h.transitioned_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') FROM base_workflow_status_history h JOIN base_workflow_bindings b ON b.id=h.binding_id WHERE b.tenant_id=$1 AND b.subject_type=$2 AND b.subject_id=$3 AND b.owner=$4 AND b.flow=$5 AND h.id>coalesce($6::bigint,0) ORDER BY h.id LIMIT $7",&[&scope.tenant(),&owner.key,&id,&owner.owner,&flow.code,&after,&(limit+1)]).await?;
        let more = rows.len() > limit as usize;
        let entries: Vec<_> = rows.into_iter().take(limit as usize).map(fact).collect();
        let next_cursor = if more {
            entries.last().map(|f| f.id)
        } else {
            None
        };
        sql.commit().await?;
        Ok(Page {
            entries,
            next_cursor,
        })
    }
}
