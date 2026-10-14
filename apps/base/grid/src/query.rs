//! Page-only projections. Every table and edge comes from its owner's scoped
//! source. The caller supplies specs, never trusted resolved columns or SQL.
use crate::{Agg, Catalog, Column, ColumnError, Join, Link, LinkKind, Table, MAX_COLUMNS};
use bilibi_database::{Db, Error};
use bilibi_tenancy::Scope;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub const MAX_ROWS: usize = 100;
pub const MAX_HOPS: usize = 128;
pub const MAX_ESTIMATED_COST: f64 = 100_000.0;
pub const STATEMENT_TIMEOUT_MS: u64 = 2_000;

#[derive(Debug)]
pub enum QueryError {
    Limit(&'static str),
    ForbiddenRoot,
    Column(String, ColumnError),
    Catalog(crate::CatalogError),
    Database(Error),
}
impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(why) => write!(f, "grid projection limit: {why}"),
            Self::ForbiddenRoot => write!(f, "grid table unavailable"),
            Self::Column(spec, why) => write!(f, "grid column {spec}: {why}"),
            Self::Catalog(error) => error.fmt(f),
            Self::Database(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for QueryError {}
impl From<tokio_postgres::Error> for QueryError {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Database(error.into())
    }
}
impl From<Error> for QueryError {
    fn from(error: Error) -> Self {
        Self::Database(error)
    }
}
impl From<crate::CatalogError> for QueryError {
    fn from(error: crate::CatalogError) -> Self {
        Self::Catalog(error)
    }
}

impl From<QueryError> for Error {
    fn from(error: QueryError) -> Self {
        match error {
            QueryError::Limit(_) | QueryError::Column(_, _) => Self::Invalid,
            QueryError::ForbiddenRoot => Self::Forbidden,
            QueryError::Catalog(error) => error.into(),
            QueryError::Database(error) => error,
        }
    }
}

/// Native JSON numbers/bools/null, with date/datetime represented by PostgreSQL's
/// ISO strings. `columns` carries the field type; consumers need not guess dates.
#[derive(Debug)]
pub struct Row {
    pub key: Value,
    pub cells: BTreeMap<String, Value>,
}
#[derive(Debug)]
pub struct Projection {
    pub columns: Vec<Column>,
    /// Authorized roots only, in the caller's page order; duplicate IDs collapse.
    pub rows: Vec<Row>,
    pub estimated_cost: Option<f64>,
}

/// Project columns over IDs the owner's list has already paged. The IDs are JSON
/// scalar keys (numbers or strings), converted to the source key's native type
/// by PostgreSQL. Missing/unauthorized IDs disappear. Grants/restrictions are
/// re-read in a read-only, repeatable-read transaction; no stale catalog or forged Column is used.
/// All statements have a local timeout, and the projection's estimated total
/// cost is checked before execution. No setting leaks back into the pool.
pub async fn project<S: AsRef<str>>(
    db: &Db,
    scope: &Scope,
    root: &str,
    ids: &[Value],
    specs: &[S],
) -> Result<Projection, QueryError> {
    if ids.len() > MAX_ROWS {
        return Err(QueryError::Limit("page exceeds 100 IDs"));
    }
    if specs.len() > MAX_COLUMNS {
        return Err(QueryError::Limit("more than 32 column specs"));
    }
    if ids
        .iter()
        .any(|id| !matches!(id, Value::Number(_) | Value::String(_)) || id.to_string().len() > 256)
    {
        return Err(QueryError::Limit(
            "keys must be scalar numbers/strings up to 256 bytes",
        ));
    }
    let mut client = db.get().await.map_err(Error::from)?;
    let tx = client
        .build_transaction()
        .read_only(true)
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    tx.batch_execute("SET LOCAL statement_timeout = '2000ms'; SET LOCAL lock_timeout = '500ms'")
        .await?;
    let catalog = Catalog::for_scope(&*tx, scope).await?;
    let root = catalog.table(root).ok_or(QueryError::ForbiddenRoot)?;
    let columns = catalog
        .resolve_all(root, specs)
        .map_err(|(spec, error)| QueryError::Column(spec, error))?;
    if columns.iter().map(|c| c.depth).sum::<usize>() > MAX_HOPS {
        return Err(QueryError::Limit("more than 128 total hops"));
    }
    if ids.is_empty() {
        return Ok(Projection {
            columns,
            rows: vec![],
            estimated_cost: None,
        });
    }
    let shape = tx
        .prepare(&format!(
            "SELECT {} FROM {} r",
            field("r", root, root.key()),
            source(root)
        ))
        .await?;
    let key_type = shape.columns()[0].type_();
    let cast = format!("{}.{}", ident(key_type.schema()), ident(key_type.name()));
    let sql = statement(&catalog, root, &columns, &cast);
    let ids = Value::Array(ids.to_vec());
    let tenant = scope.tenant();
    let user = scope.user();
    let params: &[&(dyn tokio_postgres::types::ToSql + Sync)] = &[&tenant, &user, &ids];
    let explain: Value = tx
        .query_one(&format!("EXPLAIN (FORMAT JSON) {sql}"), params)
        .await?
        .try_get(0)?;
    let cost = explain[0]["Plan"]["Total Cost"]
        .as_f64()
        .ok_or(Error::Internal)?;
    if !cost.is_finite() || cost > MAX_ESTIMATED_COST {
        return Err(QueryError::Limit("estimated query cost exceeds 100000"));
    }
    let selected = tx.query(&sql, params).await?;
    if selected.len() > MAX_ROWS {
        return Err(crate::CatalogError::Misdeclared(format!(
            "table {} source duplicated page keys",
            root.id()
        ))
        .into());
    }
    let mut keys = BTreeSet::new();
    let mut rows = Vec::with_capacity(selected.len());
    for row in selected {
        let key: Value = row.try_get(0)?;
        if !keys.insert(key.to_string()) {
            return Err(crate::CatalogError::Misdeclared(format!(
                "table {} source duplicated page keys",
                root.id()
            ))
            .into());
        }
        let mut cells = BTreeMap::new();
        for (i, column) in columns.iter().enumerate() {
            cells.insert(column.id.clone(), row.try_get(i + 1)?);
        }
        rows.push(Row { key, cells });
    }
    tx.commit().await?;
    Ok(Projection {
        columns,
        rows,
        estimated_cost: Some(cost),
    })
}

// Only catalog-owned identifiers enter SQL. Quote them even though declaration
// validation limits them to lowercase ASCII. Values always use bind parameters.
fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn field(alias: &str, table: &Table, id: &str) -> String {
    format!(
        "{alias}.{}",
        ident(table.field(id).expect("resolved field").source_key())
    )
}
fn source(table: &Table) -> String {
    format!("({})", table.source().sql)
}
fn condition(link: &Link, parent: &Table, target: &Table, p: &str, t: &str) -> String {
    match link.decl.join {
        Join::On {
            from_field,
            to_field,
        } => format!(
            "{} = {}",
            field(p, parent, from_field),
            field(t, target, to_field)
        ),
        Join::Edge(edge) => format!(
            "{} IN (SELECT e.to_key FROM ({}) e WHERE e.from_key = {})",
            field(t, target, target.key()),
            edge.sql,
            field(p, parent, parent.key())
        ),
    }
}

// Each column is a correlated scalar subquery, so independent many paths never
// multiply each other or the page. One-hops select a scalar key first: a broken
// owner declaration returning two targets fails instead of silently inflating
// aggregates. Edge duplicates are set membership, not duplicate business rows.
fn cell(catalog: &Catalog, root: &Table, column: &Column) -> String {
    let links: Vec<_> = column
        .hops
        .iter()
        .chain(column.many.iter())
        .chain(column.tail.iter())
        .collect();
    if links.is_empty() {
        return field("r", root, column.field.expect("field column").id);
    }
    let mut from = String::new();
    let mut parent = root;
    let mut p = "r".to_string();
    let mut first_condition = String::new();
    for (i, link) in links.iter().enumerate() {
        let target = catalog.table(link.to()).expect("resolved target");
        let t = format!("t{i}");
        let predicate = if link.kind() == LinkKind::One {
            // For an edge, EXISTS/set membership alone could hide two distinct
            // targets; this scalar SELECT enforces the declared cardinality.
            format!(
                "{} = (SELECT {} FROM {} lookup WHERE {})",
                field(&t, target, target.key()),
                field("lookup", target, target.key()),
                source(target),
                condition(link, parent, target, &p, "lookup")
            )
        } else {
            condition(link, parent, target, &p, &t)
        };
        if i == 0 {
            from = format!("{} {t}", source(target));
            first_condition = predicate;
        } else {
            from.push_str(&format!(" LEFT JOIN {} {t} ON {predicate}", source(target)));
        }
        p = t;
        parent = target;
    }
    let suffix = format!("FROM {from} WHERE {first_condition}");
    let value = column.field.map(|f| field(&p, parent, f.id));
    match column.agg {
        None => format!("(SELECT {} {suffix})", value.unwrap()),
        Some(Agg::Count) => {
            let many_index = column.hops.len();
            let many_table = catalog.table(column.many.unwrap().to()).unwrap();
            format!("(SELECT count({}) {suffix})", field(&format!("t{many_index}"), many_table, many_table.key()))
        }
        Some(Agg::Latest) => format!("(SELECT {} {suffix} ORDER BY {} DESC NULLS LAST, {} DESC LIMIT 1)", value.unwrap(), field(&p, parent, parent.time_field().unwrap()), field(&p, parent, parent.key())),
        Some(Agg::List) => format!("(SELECT left(string_agg(v, ', ' ORDER BY v), 300) FROM (SELECT DISTINCT left(({})::text, 300) AS v {suffix} ORDER BY v LIMIT 301) listed)", value.unwrap()),
        Some(agg) => format!("(SELECT {}({}) {suffix})", agg.key(), value.unwrap()),
    }
}
fn statement(catalog: &Catalog, root: &Table, columns: &[Column], key_cast: &str) -> String {
    let mut select = format!("to_jsonb({})", field("r", root, root.key()));
    for column in columns {
        select.push_str(&format!(
            ", COALESCE(to_jsonb({}), 'null'::jsonb)",
            cell(catalog, root, column)
        ));
    }
    // Cast input values to the server-described key type. This keeps the owner
    // key bare, allowing its index to bound the page lookup. Type identifiers
    // come from PostgreSQL metadata, never from a request.
    format!("WITH input AS (SELECT (id #>> '{{}}')::{key_cast} AS key, min(ord) AS ord FROM jsonb_array_elements($3::jsonb) WITH ORDINALITY AS x(id,ord) GROUP BY 1), actor AS (SELECT $1::bigint AS tenant, $2::bigint AS usr) SELECT {select} FROM input JOIN {} r ON {} = input.key CROSS JOIN actor ORDER BY input.ord LIMIT 101", source(root), field("r", root, root.key()))
}
