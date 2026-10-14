//! Read-only SQL console: text guards, a PostgreSQL `READ ONLY` transaction,
//! and one action row per command whatever its outcome. The record is the
//! control: an unrecorded command returns no answer.
use crate::{bounded_text, record_action, Attribution, RequestInfo};
use bilibi_database::{Db, Error, Result};
use bilibi_tenancy::Scope;
use serde_json::{json, Value};
use tokio_postgres::{error::SqlState, Client};

pub const MAX_ROWS: i64 = 1000;
pub const TIMEOUT_MS: i64 = 10_000;
const FORBIDDEN: [&str; 12] = [
    "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "TRUNCATE", "REPLACE", "GRANT",
    "REVOKE", "LOCK", "UNLOCK",
];

#[derive(Clone, Debug)]
pub struct ConsoleRequest<'a> {
    pub sql: &'a str,
    /// A saved query's name, recorded beside the text; never executed.
    pub name: Option<&'a str>,
    pub page: i64,
    pub per_page: i64,
}
#[derive(Clone, Debug)]
pub struct ConsoleResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    pub last_page: i64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Guard {
    Operator,
    Empty,
    Statement,
    Keyword,
    ReadOnlyTransaction,
}
impl Guard {
    pub fn key(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Empty => "empty",
            Self::Statement => "statement",
            Self::Keyword => "keyword",
            Self::ReadOnlyTransaction => "read_only_transaction",
        }
    }
}
/// What the record says became of the command: a count, never its rows.
#[derive(Clone, Debug)]
pub enum Outcome {
    Succeeded { row_count: i64 },
    Refused { guard: Guard, message: String },
    Failed { message: String },
}
impl Outcome {
    pub fn event(&self) -> &'static str {
        match self {
            Self::Succeeded { .. } => "database_query.executed",
            Self::Refused { .. } => "database_query.refused",
            Self::Failed { .. } => "database_query.failed",
        }
    }
}
pub struct ConsoleAnswer {
    pub outcome: Outcome,
    pub result: Option<ConsoleResult>,
    pub action_id: i64,
}

/// Operator identity is proven here and its refusal recorded, like every
/// other guard. Capability checks belong to the screen that offers the console.
pub async fn execute_readonly(
    db: &Db,
    scope: &Scope,
    request: ConsoleRequest<'_>,
    info: Option<&RequestInfo>,
) -> Result<ConsoleAnswer> {
    let mut client = db.get().await?;
    let (outcome, result) = run(&mut client, scope, &request).await?;
    let mut payload = json!({"sql": bounded_text(request.sql)});
    if let Some(name) = request.name {
        payload["name"] = Value::String(bounded_text(name));
    }
    match &outcome {
        Outcome::Succeeded { row_count } => {
            payload["result"] = json!("succeeded");
            payload["row_count"] = json!(row_count);
        }
        Outcome::Refused { guard, message } => {
            payload["result"] = json!("refused");
            payload["guard"] = json!(guard.key());
            payload["message"] = json!(bounded_text(message));
        }
        Outcome::Failed { message } => {
            payload["result"] = json!("failed");
            payload["message"] = json!(bounded_text(message));
        }
    }
    // Recorded after the read-only transaction ended, on the same connection.
    let action_id = record_action(
        &**client,
        &Attribution::from_scope(scope),
        info,
        outcome.event(),
        payload,
        false,
    )
    .await?;
    Ok(ConsoleAnswer {
        outcome,
        result,
        action_id,
    })
}

async fn run(
    client: &mut Client,
    scope: &Scope,
    request: &ConsoleRequest<'_>,
) -> Result<(Outcome, Option<ConsoleResult>)> {
    let refused = |guard: Guard, message: &str| {
        Ok((
            Outcome::Refused {
                guard,
                message: message.to_owned(),
            },
            None,
        ))
    };
    match bilibi_tenancy::prove_operator(&*client, scope).await {
        Ok(_) => {}
        Err(Error::Forbidden) => {
            return refused(
                Guard::Operator,
                "The database console is restricted to the platform operator.",
            )
        }
        Err(error) => return Err(error),
    }
    let sql = request.sql.trim().trim_end_matches(';').trim();
    if sql.is_empty() {
        return refused(Guard::Empty, "Query cannot be empty.");
    }
    let stripped = strip(sql);
    let mut words = stripped
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_uppercase);
    if !matches!(words.next().as_deref(), Some("SELECT" | "WITH")) {
        return refused(
            Guard::Statement,
            "Only SELECT or WITH queries are permitted.",
        );
    }
    if words.any(|word| FORBIDDEN.contains(&word.as_str())) {
        return refused(
            Guard::Keyword,
            "Write or DDL statements are not permitted in queries.",
        );
    }
    let page = request.page.max(1);
    let per_page = request.per_page.clamp(1, MAX_ROWS);
    match query(client, sql, page, per_page).await {
        Ok(result) => Ok((
            Outcome::Succeeded {
                row_count: result.total,
            },
            Some(result),
        )),
        Err(error) => match error.as_db_error() {
            // Text guards are advisory; PostgreSQL's read-only mode is the boundary.
            Some(db) if *db.code() == SqlState::READ_ONLY_SQL_TRANSACTION => {
                refused(Guard::ReadOnlyTransaction, db.message())
            }
            Some(db) => Ok((
                Outcome::Failed {
                    message: db.message().to_owned(),
                },
                None,
            )),
            None => Err(error.into()),
        },
    }
}

async fn query(
    client: &mut Client,
    sql: &str,
    page: i64,
    per_page: i64,
) -> std::result::Result<ConsoleResult, tokio_postgres::Error> {
    let tx = client.build_transaction().read_only(true).start().await?;
    tx.batch_execute(&format!("SET LOCAL statement_timeout = {TIMEOUT_MS}"))
        .await?;
    let total: i64 = tx
        .query_one(
            &format!("SELECT count(*) FROM ({sql}\n) AS __blb_count"),
            &[],
        )
        .await?
        .get(0);
    let columns: Vec<String> = tx
        .prepare(&format!("SELECT * FROM ({sql}\n) AS __blb_sample LIMIT 0"))
        .await?
        .columns()
        .iter()
        .map(|column| column.name().to_owned())
        .collect();
    let rows = tx
        .query(
            &format!(
                "SELECT row_to_json(__blb_view) FROM ({sql}\n) AS __blb_view LIMIT $1 OFFSET $2"
            ),
            &[&per_page, &((page - 1) * per_page)],
        )
        .await?
        .iter()
        .map(|row| {
            let mut object: Value = row.get(0);
            columns
                .iter()
                .map(|column| {
                    object
                        .get_mut(column)
                        .map(Value::take)
                        .unwrap_or(Value::Null)
                })
                .collect()
        })
        .collect();
    tx.rollback().await?;
    Ok(ConsoleResult {
        columns,
        rows,
        total,
        page,
        per_page,
        last_page: ((total + per_page - 1) / per_page).max(1),
    })
}

/// Drops string literals and comments so keyword checks see code only.
fn strip(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\'' => {
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        if chars.get(i + 1) == Some(&'\'') {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                out.push(' ');
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                out.push(' ');
                continue;
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 1;
                out.push(' ');
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}
