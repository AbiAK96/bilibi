//! Company list predicates mirror Bilimbi Core Company's scoped list reads;
//! Extension paths are resolved by Base Grid, never by page-owned joins.
use crate::{field_contributions, Company, CompanyField, CompanyFieldKind, Page, Params};
use bilibi_database::{Error, Result};
use bilibi_grid::Catalog;
use bilibi_tenancy::Scope;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use tokio_postgres::{types::ToSql, Client, GenericClient};

pub const MAX_FILTERS: usize = 10;
pub const MAX_FILTER_BYTES: usize = 8_192;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldFilter {
    pub field: String,
    pub operator: String,
    pub value: String,
}

pub(crate) struct AvailableField {
    pub definition: CompanyField,
    pub choices: Vec<bilibi_ui::Choice>,
}

pub(crate) async fn available(
    client: &(impl GenericClient + Sync),
    scope: &Scope,
    catalog: &Catalog,
    masked: &HashSet<String>,
) -> Result<Vec<AvailableField>> {
    let mut result = Vec::new();
    for (_, definition) in field_contributions::definitions()? {
        if masked.contains(definition.id)
            || bilibi_grid::scalar_expression(catalog, "companies", definition.grid_column).is_err()
        {
            continue;
        }
        let choices = match definition.kind {
            CompanyFieldKind::Choice { picklist } => {
                field_contributions::picklists()?
                    .choices(client, scope, picklist, None)
                    .await?
            }
            _ => Vec::new(),
        };
        result.push(AvailableField {
            definition,
            choices,
        });
    }
    Ok(result)
}

pub(crate) fn operators(kind: CompanyFieldKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        CompanyFieldKind::Text { .. } => &[("contains", "Contains"), ("eq", "Equals")],
        CompanyFieldKind::Date => &[
            ("eq", "On"),
            ("lt", "Before"),
            ("lte", "On or before"),
            ("gt", "After"),
            ("gte", "On or after"),
        ],
        CompanyFieldKind::Choice { .. } => &[("eq", "Equals")],
    }
}

fn filter_error(filter: &FieldFilter, field: &AvailableField) -> Option<String> {
    if !operators(field.definition.kind)
        .iter()
        .any(|(op, _)| *op == filter.operator)
    {
        return Some("Choose an operator supported by this field.".into());
    }
    let valid = match field.definition.kind {
        CompanyFieldKind::Text { .. } => !filter.value.is_empty() && filter.value.len() <= 256,
        CompanyFieldKind::Date => field_contributions::valid_date(&filter.value),
        CompanyFieldKind::Choice { .. } => field
            .choices
            .iter()
            .any(|choice| choice.value == filter.value),
    };
    (!valid).then(|| {
        match field.definition.kind {
            CompanyFieldKind::Text { .. } => "Enter text up to 256 bytes.",
            CompanyFieldKind::Date => "Enter a valid date from 0001-01-01 to 9999-12-31.",
            CompanyFieldKind::Choice { .. } => "Choose an active value from this list.",
        }
        .into()
    })
}

pub(crate) async fn list(client: &mut Client, scope: &Scope, params: &Params) -> Result<Page> {
    params.validate()?;
    bilibi_authz::authorize_key(client, scope, crate::LIST).await?;
    let tx = client
        .build_transaction()
        .read_only(true)
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    tx.batch_execute(bilibi_grid::BOUNDS).await?;
    let result = read(&tx, scope, params).await;
    match result {
        Ok(page) => {
            tx.commit().await?;
            Ok(page)
        }
        Err(error)
            if error.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED)
                || error.code() == Some(&tokio_postgres::error::SqlState::LOCK_NOT_AVAILABLE) =>
        {
            tx.rollback().await?;
            Ok(Page::limited())
        }
        Err(error) => Err(error.into()),
    }
}

// Keep database errors intact until the owner can turn timeout into a bounded
// refusal. Validation/catalog failures are handled before running statements.
async fn read(
    tx: &tokio_postgres::Transaction<'_>,
    scope: &Scope,
    params: &Params,
) -> std::result::Result<Page, ReadError> {
    let masked = field_contributions::effective_restrictions(tx, scope).await?;
    let catalog = Catalog::for_scope(tx, scope).await.map_err(Error::from)?;
    let fields = available(tx, scope, &catalog, &masked).await?;
    let mut args: Vec<Box<dyn ToSql + Sync + Send>> = vec![
        Box::new(scope.tenant()),
        Box::new(scope.user()),
        Box::new(params.status.clone()),
        Box::new(format!("%{}%", crate::organisation::escape_like(&params.q))),
    ];
    let searchable = ["name", "code", "legal_name", "email", "jurisdiction"]
        .into_iter()
        .filter(|name| !masked.contains(*name))
        .map(|name| format!("r.{name} ILIKE $4"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let search = if searchable.is_empty() {
        // Empty search stays unrestricted; when every searchable field is
        // masked, a nonempty search must match nothing without invalid SQL.
        "$4='%%'".to_owned()
    } else {
        searchable
    };
    let mut predicates = vec![
        "r.tenant_id=$1".into(),
        "r.deleted_at IS NULL".into(),
        "($3='' OR r.status=$3)".into(),
        format!("({search})"),
    ];
    let mut errors = Vec::new();
    let mut notice = String::new();
    for filter in &params.field_filters {
        let Some(field) = fields
            .iter()
            .find(|field| field.definition.id == filter.field)
        else {
            errors.push(Some(
                "This filter is unavailable and was not applied.".into(),
            ));
            notice = "An unavailable field was removed from the query.".into();
            continue;
        };
        let error = filter_error(filter, field);
        errors.push(error.clone());
        if error.is_some() {
            continue;
        }
        let expression =
            bilibi_grid::scalar_expression(&catalog, "companies", field.definition.grid_column)
                .map_err(Error::from)?;
        let operator = match filter.operator.as_str() {
            "contains" => "ILIKE",
            "eq" => "=",
            "lt" => "<",
            "lte" => "<=",
            "gt" => ">",
            "gte" => ">=",
            _ => return Err(Error::Invalid.into()),
        };
        let value = if filter.operator == "contains" {
            format!("%{}%", crate::organisation::escape_like(&filter.value))
        } else {
            filter.value.clone()
        };
        args.push(Box::new(value));
        let cast = if matches!(field.definition.kind, CompanyFieldKind::Date) {
            "::text::date"
        } else {
            "::text"
        };
        predicates.push(format!("{expression} {operator} ${}{cast}", args.len()));
    }
    let mut sort = params.sort.clone();
    let expression = if let Some(field) = fields.iter().find(|field| field.definition.id == sort) {
        bilibi_grid::scalar_expression(&catalog, "companies", field.definition.grid_column)
            .map_err(Error::from)?
    } else if ["name", "status", "jurisdiction", "created_at"].contains(&sort.as_str())
        && !masked.contains(&sort)
    {
        format!("r.{sort}")
    } else {
        sort = "name".into();
        notice = "An unavailable field was removed from the query.".into();
        "r.name".into()
    };
    let dir = if sort != params.sort {
        "asc"
    } else {
        &params.dir
    };
    let tail = format!("FROM companies r WHERE {}", predicates.join(" AND "));
    // Reserve $2 even when no contributed path uses a user-scoped source.
    let head = "WITH actor AS (SELECT $1::bigint AS tenant, $2::bigint AS usr)";
    let count = format!("{head} SELECT count(*) {tail}");
    let binds = refs(&args);
    if !admit(tx, &count, &binds).await? {
        return Ok(Page::limited());
    }
    let total = tx.query_one(&count, &binds).await?.get(0);
    args.push(Box::new(params.size));
    let limit = args.len();
    args.push(Box::new((params.page - 1) * params.size));
    let sql = format!("{head} SELECT {} {tail} ORDER BY {expression} {dir} NULLS LAST, r.id ASC LIMIT ${limit} OFFSET ${}", crate::organisation::COLUMNS, args.len());
    let binds = refs(&args);
    if !admit(tx, &sql, &binds).await? {
        return Ok(Page::limited());
    }
    let rows = tx
        .query(&sql, &binds)
        .await?
        .iter()
        .map(|row| Company::from_row(row, &masked))
        .collect();
    Ok(Page {
        rows,
        total,
        filter_errors: errors,
        notice,
        sort,
        limited: false,
    })
}

fn refs(args: &[Box<dyn ToSql + Sync + Send>]) -> Vec<&(dyn ToSql + Sync)> {
    args.iter()
        .map(|arg| arg.as_ref() as &(dyn ToSql + Sync))
        .collect()
}
async fn admit(
    tx: &tokio_postgres::Transaction<'_>,
    sql: &str,
    args: &[&(dyn ToSql + Sync)],
) -> std::result::Result<bool, tokio_postgres::Error> {
    let value: Value = tx
        .query_one(&format!("EXPLAIN (FORMAT JSON) {sql}"), args)
        .await?
        .get(0);
    Ok(value[0]["Plan"]["Total Cost"]
        .as_f64()
        .is_some_and(|cost| cost.is_finite() && cost <= bilibi_grid::MAX_ESTIMATED_COST))
}
enum ReadError {
    Database(tokio_postgres::Error),
    Other(Error),
}
impl ReadError {
    fn code(&self) -> Option<&tokio_postgres::error::SqlState> {
        match self {
            Self::Database(e) => e.code(),
            _ => None,
        }
    }
}
impl From<tokio_postgres::Error> for ReadError {
    fn from(e: tokio_postgres::Error) -> Self {
        Self::Database(e)
    }
}
impl From<Error> for ReadError {
    fn from(e: Error) -> Self {
        Self::Other(e)
    }
}
impl From<ReadError> for Error {
    fn from(e: ReadError) -> Self {
        match e {
            ReadError::Database(e) => e.into(),
            ReadError::Other(e) => e,
        }
    }
}
