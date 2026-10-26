//! Companies: the Organisation of the approved party model, its reference
//! catalogs (legal entity types, department types), departments, the tenant's
//! primary company and provisioning, mirroring Bilimbi Core Company, plus shared-UI list/create/detail pages
//! and audited inline facts. See `README.md` for the contracts.
mod agent_operations;
mod contributions;
mod departments;
mod eligibility;
mod files;
mod lifecycle;
mod organisation;
mod page_data;
mod tenant_provisioning;
pub use tenant_provisioning::TenantCompanyErrors;
mod primary;
mod reference_targets;
mod reference_types;
mod relationship_operations;
mod relationship_roles;
mod relationships;
mod web;

pub use contributions::{CAPABILITIES, CREATE, DELETE, LIST, TENANT_WIDE, UPDATE, VIEW};
// Departments, and the head directory another owner binds (K3).
pub use departments::{
    available_types as available_department_types, bind_head_directory,
    create as create_department, list as list_departments, Department, DepartmentSort,
    HeadCandidate, HeadClient, HeadDirectory, HeadFuture, Outcome as DepartmentOutcome,
};
// The company facts and locks other owners' transactions need (K9).
pub use eligibility::{
    account_eligibility, lock_account_company, lock_account_company_in, lock_writable_companies,
    AccountEligibility,
};
pub use files::{CompanyFiles, ARTIFACT_OWNER, ATTACHMENT_SUBJECT, FILES};
// A transition on another owner's transaction, and what it offers or refuses.
pub use lifecycle::{
    apply as apply_lifecycle, operations_from as lifecycle_operations, LifecycleError,
    Operation as LifecycleOperation,
};
// A company as another owner reads, authorizes or writes into it.
pub use organisation::{
    authorize_target, identity, list_tenant_company_ids, require_live, require_writable,
    require_writable_in, search, CreateOutcome, Directory, Entry as SearchEntry, Identity,
    NewCompany, SearchPage, SearchQuery, ValidCompany, RESTRICTABLE,
};
pub use primary::{
    assign, is_primary, platform_operator_company, primary_company, provision_platform_operator,
    provision_platform_operator_in, provision_tenant, AssignmentError, AssignmentStatus,
    ProvisionOutcome, Provisioned,
};
pub use reference_types::{
    list_legal_entity_types, DepartmentType, Direction as SortDirection, LegalEntityType, NewType,
    Outcome as TypeOutcome, TypeSort, TypeUpdate,
};
pub use relationship_roles::{
    ensure_defaults_if_empty as ensure_default_relationship_roles, RelationshipRole,
    RoleUpdate as RelationshipRoleUpdate,
};
pub use relationships::{
    assign as assign_relationship, Direction as RelationshipDirection, Entry as RelationshipEntry,
    Relationship, RelationshipUpdate,
};

use bilibi_database::{Change, Db, Error, MutationBoundary, Result};
use bilibi_notifications::NotificationBoundary;
use bilibi_security::{guard, restrictions, Scope, Sessions};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};
use tokio_postgres::{GenericClient, Row};

/// Frozen original migration; `004` is K1's additive organisation schema.
pub const MIGRATION: (&str, &str) = ("003-company", include_str!("../migrations/003.sql"));
pub const MIGRATION_ORGANISATION: (&str, &str) = (
    "core-company-004-organisation",
    include_str!("../migrations/004-organisation.sql"),
);
/// K5: the relationship-role catalog and directed company → party links.
pub const MIGRATION_RELATIONSHIPS: (&str, &str) = (
    "core-company-005-relationships",
    include_str!("../migrations/005-relationships.sql"),
);
/// K15: the scoped foreign key behind Authz's company-owned custom roles.
pub const MIGRATION_ROLE_OWNER: (&str, &str) = (
    "core-company-006-authz-role-owner",
    include_str!("../migrations/006-authz-role-owner.sql"),
);
/// Global search: the stored text-search vector Base Search matches on.
pub const MIGRATION_SEARCH: (&str, &str) = (
    "core-company-007-search",
    include_str!("../migrations/007-search.sql"),
);
pub const MIGRATION_CUSTOM_FIELDS: (&str, &str) = (
    "core-company-008-custom-fields",
    include_str!("../migrations/008-custom-fields.sql"),
);
pub const MIGRATION_CUSTOM_FIELDS_COLUMN_NAME: (&str, &str) = (
    "core-company-009-custom-fields-column-name",
    include_str!("../migrations/009-custom-fields-column-name.sql"),
);
pub const MASK: &str = "••••";
/// The registry service id of the Companies facade.
pub const SERVICE: &str = "core/company.companies";

/// One refusal of a submitted value, in the words a form shows beside the field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}
impl FieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}
/// Why a write did not happen. `NotFound`, `Archived`, `InUse` and `Stale`
/// are outcomes a page names; `Access` is denial or a database fault.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("Not found")]
    NotFound,
    #[error("This company is archived and read-only")]
    Archived,
    #[error("In use")]
    InUse,
    #[error("The record changed. Reload before saving")]
    Stale,
    #[error("Invalid input")]
    Invalid(Vec<FieldError>),
    #[error(transparent)]
    Access(#[from] Error),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Params {
    pub page: i64,
    pub size: i64,
    pub sort: String,
    pub dir: String,
    pub q: String,
    pub status: String,
    #[serde(skip)]
    pub custom_filters: Vec<CustomFilter>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CustomFilter {
    pub field: String,
    pub operator: String,
    pub value: String,
}
impl Default for Params {
    fn default() -> Self {
        Self {
            page: 1,
            size: 25,
            sort: "name".into(),
            dir: "asc".into(),
            q: String::new(),
            status: String::new(),
            custom_filters: Vec::new(),
        }
    }
}
impl Params {
    pub fn validate(&self) -> Result<()> {
        if !(1..=1_000_000).contains(&self.page)
            || ![25, 50, 100, 300].contains(&self.size)
            || (!(["name", "status", "jurisdiction", "created_at"].contains(&self.sort.as_str()))
                && !(self.sort.starts_with("cf_")
                    && self.sort.len() <= 68
                    && self.sort[3..]
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')))
            || !["asc", "desc"].contains(&self.dir.as_str())
            || self.q.chars().count() > 255
            || !["", "active", "suspended", "pending", "archived"].contains(&self.status.as_str())
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

/// The read model of one company as the reader may see it: a restricted
/// field carries `MASK` instead of its value (`RESTRICTABLE`).
#[derive(Clone, Debug)]
pub struct Company {
    pub id: i64,
    pub tenant_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub code: String,
    pub status: String,
    pub legal_name: Option<String>,
    pub registration_number: Option<String>,
    pub tax_id: String,
    pub legal_entity_type_id: Option<i64>,
    pub jurisdiction: Option<String>,
    pub email: String,
    pub phone: String,
    pub website: Option<String>,
    pub revenue: String,
    pub version: i64,
    pub custom: serde_json::Value,
}
impl Company {
    /// Organisation identity of this company, preserving existing record IDs.
    /// The ID alone does not prove tenant ownership or authorization.
    pub fn organisation_id(&self) -> Result<bilibi_party_ids::OrganisationId> {
        bilibi_party_ids::OrganisationId::new(self.id).map_err(|_| Error::Invalid)
    }
    /// The name a header shows: the legal name when there is one.
    pub fn display_name(&self) -> String {
        organisation::display_name(self.legal_name.as_deref(), &self.name)
    }
    pub fn archived(&self) -> bool {
        self.status == "archived"
    }
    pub(crate) fn from_row(row: &Row, masked: &HashSet<String>) -> Self {
        let read = |field: &str| {
            if masked.contains(field) {
                MASK.to_owned()
            } else {
                row.get(field)
            }
        };
        let read_opt = |field: &str| {
            if masked.contains(field) {
                Some(MASK.to_owned())
            } else {
                row.get(field)
            }
        };
        let mut custom: serde_json::Value =
            row.try_get("custom_fields").unwrap_or_else(|_| json!({}));
        if let Some(object) = custom.as_object_mut() {
            for field in masked.iter().filter_map(|field| field.strip_prefix("cf_")) {
                object.remove(field);
            }
        }
        Self {
            id: row.get("id"),
            tenant_id: row.get("tenant_id"),
            parent_id: row.get("parent_id"),
            name: row.get("name"),
            code: row.get("code"),
            status: row.get("status"),
            legal_name: row.get("legal_name"),
            registration_number: read_opt("registration_number"),
            tax_id: read("tax_id"),
            legal_entity_type_id: row.get("legal_entity_type_id"),
            jurisdiction: read_opt("jurisdiction"),
            email: read("email"),
            phone: read("phone"),
            website: read_opt("website"),
            revenue: read("revenue"),
            version: row.get("version"),
            custom,
        }
    }
}
pub struct Page {
    pub rows: Vec<Company>,
    pub total: i64,
    /// The Grid cost admission bound refused this list query before execution.
    pub(crate) limited: bool,
}
use organisation::COLUMNS;

pub async fn list(
    client: &mut tokio_postgres::Client,
    scope: &Scope,
    params: &Params,
) -> Result<Page> {
    let tx = client
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    tx.batch_execute("SET LOCAL statement_timeout = '2000ms'; SET LOCAL lock_timeout = '500ms'; SET LOCAL TimeZone = 'UTC'")
        .await?;
    let page = list_with_catalog(&tx, scope, params, None, &[]).await?;
    tx.commit().await?;
    Ok(page)
}

/// Company list query with current, scoped custom-field metadata. The owner
/// expression builder binds JSON keys; only fixed operators and catalog types
/// become SQL syntax. Mirrors Bilimbi Core Company’s custom-field list query.
pub async fn list_with_catalog(
    client: &tokio_postgres::Transaction<'_>,
    scope: &Scope,
    params: &Params,
    catalog: Option<&bilibi_grid::Catalog>,
    definitions: &[bilibi_custom_fields::Definition],
) -> Result<Page> {
    params.validate()?;
    bilibi_authz::authorize_key(client, scope, LIST).await?;
    let masked = restrictions(client, scope).await?;
    let query = format!("%{}%", organisation::escape_like(&params.q));
    let searchable = ["name", "code", "legal_name", "email", "jurisdiction"]
        .into_iter()
        .filter(|field| !masked.contains(*field))
        .map(|field| format!("{field} ILIKE $3"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut args: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = vec![
        Box::new(scope.tenant()),
        Box::new(params.status.clone()),
        Box::new(query),
    ];
    let mut conditions = vec![
        "tenant_id=$1".to_owned(),
        "deleted_at IS NULL".to_owned(),
        "($2='' OR status=$2)".to_owned(),
        format!("({searchable})"),
    ];
    if params.custom_filters.len() > 10 {
        return Err(Error::Invalid);
    }
    for condition in &params.custom_filters {
        let Some(definition) = definitions.iter().find(|definition| {
            format!("cf_{}", definition.key) == condition.field && !definition.archived
        }) else {
            return Err(Error::Invalid);
        };
        let Some(catalog) = catalog else {
            return Err(Error::Invalid);
        };
        let expression = bilibi_grid::query::field_expression(
            catalog,
            "companies",
            &condition.field,
            "companies",
            args.len(),
        )
        .ok_or(Error::Forbidden)?;
        if masked.contains(&condition.field) {
            return Err(Error::Forbidden);
        }
        if definition.field_type == bilibi_custom_fields::FieldType::MultiChoice {
            if condition.operator != "contains" || !choice_code(definition, &condition.value) {
                return Err(Error::Invalid);
            }
            args.push(Box::new(expression.bind_values[0].clone()));
            let key_pos = args.len();
            args.push(Box::new(condition.value.clone()));
            conditions.push(format!(
            "jsonb_extract_path(companies.custom_fields, ${key_pos}::text) @> to_jsonb(${}::text)",
                args.len()
            ));
            continue;
        }
        if definition.field_type == bilibi_custom_fields::FieldType::Decimal
            && !valid_decimal_filter(&condition.value)
        {
            return Err(Error::Invalid);
        }
        let normalized_temporal = match definition.field_type {
            bilibi_custom_fields::FieldType::Date => {
                Some(normalize_date_filter(&condition.value).ok_or(Error::Invalid)?)
            }
            bilibi_custom_fields::FieldType::DateTime => {
                Some(normalize_datetime_filter(&condition.value).ok_or(Error::Invalid)?)
            }
            _ => None,
        };
        let operator = match (definition.field_type, condition.operator.as_str()) {
            (
                bilibi_custom_fields::FieldType::Text | bilibi_custom_fields::FieldType::LongText,
                "contains",
            ) => "ILIKE",
            (
                bilibi_custom_fields::FieldType::Text | bilibi_custom_fields::FieldType::LongText,
                "equals",
            ) => "=",
            (
                bilibi_custom_fields::FieldType::Integer
                | bilibi_custom_fields::FieldType::Decimal
                | bilibi_custom_fields::FieldType::Date
                | bilibi_custom_fields::FieldType::DateTime,
                "eq",
            ) => "=",
            (
                bilibi_custom_fields::FieldType::Integer
                | bilibi_custom_fields::FieldType::Decimal
                | bilibi_custom_fields::FieldType::Date
                | bilibi_custom_fields::FieldType::DateTime,
                "gt",
            ) => ">",
            (
                bilibi_custom_fields::FieldType::Integer
                | bilibi_custom_fields::FieldType::Decimal
                | bilibi_custom_fields::FieldType::Date
                | bilibi_custom_fields::FieldType::DateTime,
                "gte",
            ) => ">=",
            (
                bilibi_custom_fields::FieldType::Integer
                | bilibi_custom_fields::FieldType::Decimal
                | bilibi_custom_fields::FieldType::Date
                | bilibi_custom_fields::FieldType::DateTime,
                "lt",
            ) => "<",
            (
                bilibi_custom_fields::FieldType::Integer
                | bilibi_custom_fields::FieldType::Decimal
                | bilibi_custom_fields::FieldType::Date
                | bilibi_custom_fields::FieldType::DateTime,
                "lte",
            ) => "<=",
            (
                bilibi_custom_fields::FieldType::Boolean | bilibi_custom_fields::FieldType::Choice,
                "equals",
            ) => "=",
            _ => return Err(Error::Invalid),
        };
        if definition.field_type == bilibi_custom_fields::FieldType::Choice
            && !choice_code(definition, &condition.value)
        {
            return Err(Error::Invalid);
        }
        let value = if let Some(value) = normalized_temporal {
            value
        } else if condition.operator == "contains" {
            format!("%{}%", organisation::escape_like(&condition.value))
        } else {
            condition.value.clone()
        };
        for key in expression.bind_values {
            args.push(Box::new(key));
        }
        let value_pos = args.len() + 1;
        args.push(Box::new(value));
        let cast = match definition.field_type {
            bilibi_custom_fields::FieldType::Integer => "::text::bigint",
            bilibi_custom_fields::FieldType::Decimal => "::text::numeric",
            bilibi_custom_fields::FieldType::Date => "::text::date",
            bilibi_custom_fields::FieldType::DateTime => "::text::timestamptz",
            bilibi_custom_fields::FieldType::Boolean => "::text::boolean",
            _ => "::text",
        };
        conditions.push(format!("{} {operator} ${value_pos}{cast}", expression.sql));
    }
    let filter = conditions.join(" AND ");
    let count_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = args
        .iter()
        .map(|arg| arg.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    let count_sql = format!("SELECT count(*) FROM companies WHERE {filter}");
    if !within_grid_cost(client, &count_sql, &count_refs).await? {
        return Ok(Page {
            rows: Vec::new(),
            total: 0,
            limited: true,
        });
    }
    client.batch_execute("SAVEPOINT company_grid_count").await?;
    let count = client.query_one(&count_sql, &count_refs).await;
    let total = match count {
        Ok(row) => {
            client
                .batch_execute("RELEASE SAVEPOINT company_grid_count")
                .await?;
            row.get(0)
        }
        Err(error) if error.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED) => {
            client
                .batch_execute("ROLLBACK TO SAVEPOINT company_grid_count; RELEASE SAVEPOINT company_grid_count")
                .await?;
            return Ok(Page {
                rows: Vec::new(),
                total: 0,
                limited: true,
            });
        }
        Err(error) => {
            client
                .batch_execute("ROLLBACK TO SAVEPOINT company_grid_count; RELEASE SAVEPOINT company_grid_count")
                .await?;
            return Err(error.into());
        }
    };
    let sort_expression = if params.sort.starts_with("cf_") {
        let allowed = definitions.iter().any(|definition| {
            format!("cf_{}", definition.key) == params.sort && !definition.archived
        });
        if !allowed || masked.contains(&params.sort) {
            "name".into()
        } else if let Some(catalog) = catalog {
            match bilibi_grid::query::field_expression(
                catalog,
                "companies",
                &params.sort,
                "companies",
                args.len(),
            ) {
                Some(expression) => {
                    for key in expression.bind_values {
                        args.push(Box::new(key));
                    }
                    expression.sql
                }
                None => "name".into(),
            }
        } else {
            "name".into()
        }
    } else if masked.contains(&params.sort) {
        "name".to_owned()
    } else {
        params.sort.clone()
    };
    // Only validated enum values reach the two SQL identifiers below.
    let limit_pos = args.len() + 1;
    args.push(Box::new(params.size));
    let offset = (params.page - 1) * params.size;
    args.push(Box::new(offset));
    let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = args
        .iter()
        .map(|arg| arg.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    let sql = format!("SELECT {COLUMNS} FROM companies WHERE {filter} ORDER BY {sort_expression} {} , id {} LIMIT ${limit_pos} OFFSET ${}", params.dir, params.dir, limit_pos+1);
    if !within_grid_cost(client, &sql, &refs).await? {
        return Ok(Page {
            rows: Vec::new(),
            total: 0,
            limited: true,
        });
    }
    client.batch_execute("SAVEPOINT company_grid_rows").await?;
    let selected = client.query(&sql, &refs).await;
    let selected = match selected {
        Ok(rows) => {
            client
                .batch_execute("RELEASE SAVEPOINT company_grid_rows")
                .await?;
            rows
        }
        Err(error) if error.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED) => {
            client
                .batch_execute(
                    "ROLLBACK TO SAVEPOINT company_grid_rows; RELEASE SAVEPOINT company_grid_rows",
                )
                .await?;
            return Ok(Page {
                rows: Vec::new(),
                total: 0,
                limited: true,
            });
        }
        Err(error) => {
            client
                .batch_execute(
                    "ROLLBACK TO SAVEPOINT company_grid_rows; RELEASE SAVEPOINT company_grid_rows",
                )
                .await?;
            return Err(error.into());
        }
    };
    let rows = selected
        .iter()
        .map(|row| Company::from_row(row, &masked))
        .collect();
    Ok(Page {
        rows,
        total,
        limited: false,
    })
}

async fn within_grid_cost(
    client: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<bool> {
    client.batch_execute("SAVEPOINT company_grid_cost").await?;
    let estimate = client
        .query_one(&format!("EXPLAIN (FORMAT JSON) {sql}"), params)
        .await;
    let estimate = match estimate {
        Ok(row) => {
            client
                .batch_execute("RELEASE SAVEPOINT company_grid_cost")
                .await?;
            row.try_get(0)?
        }
        Err(error) if error.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED) => {
            client
                .batch_execute(
                    "ROLLBACK TO SAVEPOINT company_grid_cost; RELEASE SAVEPOINT company_grid_cost",
                )
                .await?;
            return Ok(false);
        }
        Err(error) => {
            client
                .batch_execute(
                    "ROLLBACK TO SAVEPOINT company_grid_cost; RELEASE SAVEPOINT company_grid_cost",
                )
                .await?;
            return Err(error.into());
        }
    };
    let explain: serde_json::Value = estimate;
    let Some(cost) = explain[0]["Plan"]["Total Cost"].as_f64() else {
        return Err(Error::Internal);
    };
    Ok(cost.is_finite() && cost <= bilibi_grid::query::MAX_ESTIMATED_COST)
}

fn choice_code(definition: &bilibi_custom_fields::Definition, code: &str) -> bool {
    definition.options.as_array().is_some_and(|options| {
        options
            .iter()
            .any(|option| option.get("code").and_then(serde_json::Value::as_str) == Some(code))
    })
}

/// Accept exact base-10 values that PostgreSQL `numeric` can parse without
/// exponent expansion. The URL value is already capped at 255 bytes, so this
/// also stays well inside PostgreSQL's precision and scale limits.
pub(crate) fn valid_decimal_filter(value: &str) -> bool {
    if value.is_empty() || value.len() > 255 {
        return false;
    }
    let unsigned = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    if unsigned.is_empty() {
        return false;
    }
    let mut parts = unsigned.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some() {
        return false;
    }
    let whole_digits = whole.bytes().all(|byte| byte.is_ascii_digit());
    let fraction_digits = fraction.is_none_or(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    });
    let has_digit = whole.bytes().any(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|digits| digits.bytes().any(|byte| byte.is_ascii_digit()));
    (whole_digits || whole.is_empty()) && fraction_digits && has_digit
}

pub(crate) fn normalize_date_filter(value: &str) -> Option<String> {
    let date = value.parse::<jiff::civil::Date>().ok()?;
    (date.year() > 0 && date.year() <= 9999 && date.to_string() == value).then(|| date.to_string())
}

pub(crate) fn normalize_datetime_filter(value: &str) -> Option<String> {
    if value.contains(['[', ']']) {
        return None;
    }
    let timestamp = value.parse::<jiff::Timestamp>().ok()?;
    let year = timestamp.to_zoned(jiff::tz::TimeZone::UTC).date().year();
    (year > 0 && year <= 9999).then(|| timestamp.to_string())
}

pub async fn detail(
    client: &impl GenericClient,
    scope: &Scope,
    id: i64,
) -> Result<(Company, HashSet<String>)> {
    bilibi_authz::authorize_key(client, scope, VIEW).await?;
    let masked = restrictions(client, scope).await?;
    let row = client.query_opt(&format!("SELECT {COLUMNS} FROM companies WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL"), &[&scope.tenant(),&id]).await?.ok_or(Error::NotFound)?;
    Ok((Company::from_row(&row, &masked), masked))
}

/// The columns whose empty value is stored as NULL.
const OPTIONAL: [&str; 4] = [
    "legal_name",
    "registration_number",
    "jurisdiction",
    "website",
];
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub id: i64,
    pub field: String,
    pub value: String,
    pub version: i64,
}
impl Edit {
    pub fn validate(&self) -> Result<()> {
        if let Some(key) = self.field.strip_prefix("cf_") {
            let key_valid = !key.is_empty()
                && key.len() <= 63
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
            let valid_json = serde_json::from_str::<serde_json::Value>(&self.value).is_ok();
            return if key_valid && valid_json && self.id > 0 && self.version > 0 {
                Ok(())
            } else {
                Err(Error::Invalid)
            };
        }
        let value = self.value.trim();
        let valid = match self.field.as_str() {
            "name" => !value.is_empty() && value.chars().count() <= 160,
            "code" => !value.is_empty() && value.chars().count() <= 255,
            "email" => {
                self.value.len() <= 254 && (value.is_empty() || organisation::email_shaped(value))
            }
            "phone" | "tax_id" => self.value.chars().count() <= 64,
            "legal_name" | "registration_number" | "website" => value.chars().count() <= 255,
            "jurisdiction" => {
                value.is_empty()
                    || (value.len() == 2 && value.bytes().all(|b| b.is_ascii_alphabetic()))
            }
            "revenue" => self.value.parse::<i64>().is_ok_and(|v| v >= 0),
            _ => false,
        };
        if valid && self.id > 0 && self.version > 0 {
            Ok(())
        } else {
            Err(Error::Invalid)
        }
    }
}

pub async fn save(
    db: &Db,
    token: &str,
    edit: &Edit,
    notifications: &NotificationBoundary,
    mutations: &MutationBoundary,
    sessions: &Sessions,
    custom_fields: &bilibi_custom_fields::CustomFields,
) -> Result<()> {
    edit.validate()?;
    let mut client = db.get().await?;
    let scope = guard(&**client, token, sessions).await?;
    let mut tx = mutations
        .begin(&mut client, scope.mutation_context("company.update"))
        .await?;
    let scope = guard(tx.sql(), token, sessions).await?;
    organisation::authorize_target(tx.sql(), &scope, edit.id, UPDATE).await?;
    if restrictions(tx.sql(), &scope).await?.contains(&edit.field) {
        return Err(Error::Forbidden);
    }
    let id = edit.id;
    let version = edit.version;
    let field = edit.field.clone();
    let tenant = scope.tenant();
    let value = match field.as_str() {
        "jurisdiction" => {
            let iso = edit.value.trim().to_ascii_uppercase();
            if !iso.is_empty() && !bilibi_geonames::country_known(tx.sql(), &iso).await? {
                return Err(Error::Invalid);
            }
            iso
        }
        "code" => {
            let code = edit.value.trim().to_owned();
            organisation::lock_code(tx.sql(), tenant, &code).await?;
            let taken = tx
                .sql()
                .query_opt(
                    "SELECT id FROM companies WHERE tenant_id=$1 AND code=$2 AND id<>$3",
                    &[&tenant, &code, &id],
                )
                .await?
                .is_some();
            if taken {
                return Err(Error::Invalid);
            }
            code
        }
        "name" | "legal_name" | "registration_number" | "website" => edit.value.trim().to_owned(),
        _ => edit.value.clone(),
    };
    if let Some(key) = field.strip_prefix("cf_").map(str::to_owned) {
        let row = tx.sql().query_opt(
            "SELECT custom_fields,version,status FROM companies WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE",
            &[&tenant, &id],
        ).await?.ok_or(Error::NotFound)?;
        if row.get::<_, i64>(1) != version {
            return Err(Error::Conflict);
        }
        if row.get::<_, String>(2) == "archived" {
            return Err(Error::Forbidden);
        }
        let old_custom: serde_json::Value = row.get(0);
        let old_value = old_custom
            .get(&key)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let submitted: serde_json::Value =
            serde_json::from_str(&edit.value).map_err(|_| Error::Invalid)?;
        let patch = serde_json::json!({ (key.clone()): submitted });
        let merged = match custom_fields
            .validate(tx.sql(), &scope, "core.company", &patch, &old_custom, false)
            .await?
        {
            Ok(value) => value,
            Err(_) => return Err(Error::Invalid),
        };
        let new_value = merged.get(&key).cloned().unwrap_or(serde_json::Value::Null);
        tx.mutate(move |sql| Box::pin(async move {
            sql.execute(
                "UPDATE companies SET custom_fields=$1,version=version+1,updated_at=now() WHERE tenant_id=$2 AND id=$3",
                &[&merged, &tenant, &id],
            ).await?;
            Ok(((), vec![Change { kind: "company".into(), id,
                old: json!({"custom": {key.clone(): old_value}}),
                new: json!({"custom": {key: new_value}}),
            }]))
        })).await?;
        notifications
            .record(&mut tx, &scope, "Company updated")
            .await?;
        return tx.commit().await;
    }
    tx.mutate(move |sql| Box::pin(async move {
        let row = sql.query_opt(&format!("SELECT {}::text,version,status FROM companies WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE",field), &[&tenant,&id]).await?.ok_or(Error::NotFound)?;
        if row.get::<_, i64>(1) != version { return Err(Error::Conflict); }
        if row.get::<_, String>(2) == "archived" { return Err(Error::Forbidden); }
        let old: Option<String> = row.get(0);
        let (old, new) = if field == "revenue" {
            let value = value.parse::<i64>().map_err(|_| Error::Invalid)?;
            sql.execute("UPDATE companies SET revenue=$1,version=version+1,updated_at=now() WHERE tenant_id=$2 AND id=$3", &[&value,&tenant,&id]).await?;
            (json!(old.and_then(|old| old.parse::<i64>().ok())), json!(value))
        } else if OPTIONAL.contains(&field.as_str()) {
            sql.execute(&format!("UPDATE companies SET {}=NULLIF($1,''),version=version+1,updated_at=now() WHERE tenant_id=$2 AND id=$3",field), &[&value,&tenant,&id]).await?;
            (json!(old), json!(Some(value.as_str()).filter(|v| !v.is_empty())))
        } else {
            sql.execute(&format!("UPDATE companies SET {}=$1,version=version+1,updated_at=now() WHERE tenant_id=$2 AND id=$3",field), &[&value,&tenant,&id]).await?;
            (json!(old), json!(value))
        };
        Ok(((), vec![Change { kind: "company".into(), id, old: json!({&field:old}), new: json!({&field:new}) }]))
    })).await?;
    notifications
        .record(&mut tx, &scope, "Company updated")
        .await?;
    tx.commit().await
}

/// The Companies facade service (`core/company.companies`): every write
/// opens one audited mutation transaction, calls the owning module and
/// commits only a saved outcome.
pub struct CompanyService {
    sessions: Arc<Sessions>,
    db: Db,
    notifications: Arc<NotificationBoundary>,
    mutations: Arc<MutationBoundary>,
    proofs: Arc<bilibi_tenancy::PartyProofs>,
    directory: Arc<bilibi_principal_directory::PrincipalDirectory>,
    custom_fields: Arc<bilibi_custom_fields::CustomFields>,
}
impl CompanyService {
    pub async fn save(&self, token: &str, edit: &Edit) -> Result<()> {
        save(
            &self.db,
            token,
            edit,
            &self.notifications,
            &self.mutations,
            &self.sessions,
            &self.custom_fields,
        )
        .await
    }
    /// `create_company/3`: a refused create rolls back and reports its fields.
    pub async fn create_company(&self, scope: &Scope, new: &NewCompany) -> Result<CreateOutcome> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(&mut client, scope.mutation_context("company.create"))
            .await?;
        let outcome = organisation::create(&mut tx, scope, new, &self.custom_fields).await?;
        match &outcome {
            CreateOutcome::Created(_) => tx.commit().await?,
            CreateOutcome::Refused(_) => tx.rollback().await?,
        }
        Ok(outcome)
    }
    pub async fn create_legal_entity_type(
        &self,
        scope: &Scope,
        new: &reference_types::NewType,
    ) -> Result<reference_types::Outcome<reference_types::LegalEntityType>> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.legal_entity_type.create"),
            )
            .await?;
        let outcome = reference_types::create_legal_entity_type(&mut tx, scope, new).await?;
        finish(tx, matches!(outcome, reference_types::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    pub async fn update_legal_entity_type(
        &self,
        scope: &Scope,
        id: i64,
        update: &reference_types::TypeUpdate,
    ) -> std::result::Result<reference_types::Outcome<reference_types::LegalEntityType>, WriteError>
    {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.legal_entity_type.update"),
            )
            .await?;
        let outcome = reference_types::update_legal_entity_type(&mut tx, scope, id, update).await?;
        finish(tx, matches!(outcome, reference_types::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    pub async fn toggle_legal_entity_type(
        &self,
        scope: &Scope,
        id: i64,
    ) -> std::result::Result<reference_types::LegalEntityType, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.legal_entity_type.update"),
            )
            .await?;
        let saved = reference_types::toggle_legal_entity_type(&mut tx, scope, id).await?;
        tx.commit().await?;
        Ok(saved)
    }
    pub async fn delete_legal_entity_type(
        &self,
        scope: &Scope,
        id: i64,
    ) -> std::result::Result<(), WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.legal_entity_type.delete"),
            )
            .await?;
        reference_types::delete_legal_entity_type(&mut tx, scope, id).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn create_department_type(
        &self,
        scope: &Scope,
        new: &reference_types::NewType,
    ) -> Result<reference_types::Outcome<reference_types::DepartmentType>> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.department_type.create"),
            )
            .await?;
        let outcome = reference_types::create_department_type(&mut tx, scope, new).await?;
        finish(tx, matches!(outcome, reference_types::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    pub async fn update_department_type(
        &self,
        scope: &Scope,
        id: i64,
        update: &reference_types::TypeUpdate,
    ) -> std::result::Result<reference_types::Outcome<reference_types::DepartmentType>, WriteError>
    {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.department_type.update"),
            )
            .await?;
        let outcome = reference_types::update_department_type(&mut tx, scope, id, update).await?;
        finish(tx, matches!(outcome, reference_types::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    pub async fn toggle_department_type(
        &self,
        scope: &Scope,
        id: i64,
    ) -> std::result::Result<reference_types::DepartmentType, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.department_type.update"),
            )
            .await?;
        let saved = reference_types::toggle_department_type(&mut tx, scope, id).await?;
        tx.commit().await?;
        Ok(saved)
    }
    pub async fn delete_department_type(
        &self,
        scope: &Scope,
        id: i64,
    ) -> std::result::Result<(), WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.department_type.delete"),
            )
            .await?;
        reference_types::delete_department_type(&mut tx, scope, id).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn create_department(
        &self,
        scope: &Scope,
        company_id: i64,
        department_type_id: i64,
        status: &str,
    ) -> std::result::Result<departments::Outcome, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.department.create"),
            )
            .await?;
        let outcome =
            departments::create(&mut tx, scope, company_id, department_type_id, status).await?;
        finish(tx, matches!(outcome, departments::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    pub async fn update_department_status(
        &self,
        scope: &Scope,
        company_id: i64,
        department_id: i64,
        status: &str,
        version: i64,
    ) -> std::result::Result<departments::Outcome, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.department.update"),
            )
            .await?;
        let outcome =
            departments::update_status(&mut tx, scope, company_id, department_id, status, version)
                .await?;
        finish(tx, matches!(outcome, departments::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    /// Appoints or clears a department's head from the bound
    /// `departments::HeadDirectory`, with the version the dialog loaded.
    pub async fn update_department_head(
        &self,
        scope: &Scope,
        company_id: i64,
        department_id: i64,
        head_affiliation_id: Option<i64>,
        version: i64,
    ) -> std::result::Result<departments::Outcome, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.department.update"),
            )
            .await?;
        let outcome = departments::update_head(
            &mut tx,
            scope,
            company_id,
            department_id,
            head_affiliation_id,
            version,
        )
        .await?;
        finish(tx, matches!(outcome, departments::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    pub async fn delete_department(
        &self,
        scope: &Scope,
        company_id: i64,
        department_id: i64,
    ) -> std::result::Result<(), WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.department.delete"),
            )
            .await?;
        departments::delete(&mut tx, scope, company_id, department_id).await?;
        tx.commit().await?;
        Ok(())
    }
    /// Assigns the tenant's primary company (`transfer` moves it). Requires
    /// `admin.company.update` on the target as `authorize_target` decides.
    pub async fn assign_primary_company(
        &self,
        scope: &Scope,
        company_id: i64,
        transfer: bool,
    ) -> std::result::Result<AssignmentStatus, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.primary.assign"),
            )
            .await?;
        organisation::authorize_target(tx.sql(), scope, company_id, UPDATE).await?;
        let status = primary::assign(&mut tx, scope.tenant(), company_id, transfer).await?;
        tx.commit().await?;
        Ok(status)
    }
}
async fn finish(tx: bilibi_database::MutationTransaction<'_>, saved: bool) -> Result<()> {
    if saved {
        tx.commit().await
    } else {
        tx.rollback().await
    }
}

inventory::submit! {
    bilibi_module_registry::Module {
        id:"core/company",layer:bilibi_module_registry::Layer::Core,
        dependencies:&["base/database","base/security","base/ui","base/notifications","base/party_ids","base/session","base/menu","base/grid","base/search","base/dashboard","base/authz","base/tenancy","base/audit","base/agent_api","base/datetime","base/locale","base/principal_directory","base/media","base/artifacts","base/custom_fields","core/party","core/geonames","core/person"],
        migrations:&[MIGRATION, MIGRATION_ORGANISATION, MIGRATION_RELATIONSHIPS, MIGRATION_ROLE_OWNER, MIGRATION_SEARCH, MIGRATION_CUSTOM_FIELDS, MIGRATION_CUSTOM_FIELDS_COLUMN_NAME],
        services: &[
            bilibi_module_registry::Service { id: SERVICE, factory: |services| {
                let db = services.get::<Db>("base/database.pool")?.as_ref().clone();
                // Company is the Organisation owner: it proves targets for
                // Tenancy and reads for the agent operations.
                let proofs = services.get::<bilibi_tenancy::PartyProofs>("base/tenancy.parties")?;
                proofs.bind_organisations(Arc::new(organisation::Provider))?;
                proofs.bind_directory(Arc::new(organisation::Directory))?;
                let directory = services.get::<bilibi_principal_directory::PrincipalDirectory>("base/principal_directory.directory")?;
                let custom_fields = services.get::<bilibi_custom_fields::CustomFields>(bilibi_custom_fields::SERVICE)?;
                agent_operations::bind(db.clone())?;
                agent_operations::bind_custom_fields(custom_fields.clone())?;
                relationship_operations::bind(db.clone(), proofs.clone(), directory.clone())?;
                Ok(Arc::new(CompanyService {
                    sessions: services.get("base/session.sessions")?, mutations: services.get("base/database.mutations")?,
                    db, notifications: services.get("base/notifications.sink")?, proofs, directory,
                    custom_fields,
                }))
            } },
            bilibi_module_registry::Service { id: "core/company.pages", factory: |services| Ok(Arc::new(bilibi_ui::page::PageService(Arc::new(web::Pages {
                db: services.get::<Db>("base/database.pool")?.as_ref().clone(), company: services.get(SERVICE)?, views: services.get(bilibi_grid::PAGE_VIEWS)?, datetime: services.get("base/datetime.display")?, locales: services.get("base/locale.locales")?, record_panels: web::record_panels()?,
            })))) },
            bilibi_module_registry::Service { id: web::relationships::SERVICE, factory: |services| Ok(Arc::new(bilibi_ui::page::PageService(Arc::new(web::relationships::RelationshipPages {
                db: services.get::<Db>("base/database.pool")?.as_ref().clone(), company: services.get(SERVICE)?, persons: services.get(bilibi_person::SERVICE)?,
                contracts: services.get(bilibi_agent_api::page::SERVICE)?,
            })))) },
        ],
    }
}

/// K5: the relationship-role catalog and the directed links of the party
/// model (`relationship_roles`, `relationships`). Each write is one audited
/// mutation transaction; a refusal is an outcome, never an `Error`.
impl CompanyService {
    /// The tenant's roles. A tenant whose catalog is empty (one created
    /// by raw SQL after the migration ran, as the walkthrough fixture does)
    /// first receives the contributed
    /// defaults by code; an administered catalog is never touched here.
    pub async fn relationship_roles(
        &self,
        scope: &Scope,
        include_retired: bool,
    ) -> Result<Vec<relationship_roles::RelationshipRole>> {
        let client = self.db.get().await?;
        relationship_roles::ensure_defaults_if_empty(&**client, scope.tenant()).await?;
        relationship_roles::list(&**client, scope, include_retired).await
    }
    pub async fn relationship_role(
        &self,
        scope: &Scope,
        id: bilibi_party::RelationshipRoleId,
    ) -> Result<relationship_roles::RelationshipRole> {
        let client = self.db.get().await?;
        relationship_roles::get(&**client, scope, id).await
    }
    pub async fn create_relationship_role(
        &self,
        scope: &Scope,
        input: &bilibi_party::RelationshipRoleInput,
    ) -> Result<reference_types::Outcome<relationship_roles::RelationshipRole>> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.relationship_role.create"),
            )
            .await?;
        let outcome = relationship_roles::create(&mut tx, scope, input).await?;
        finish(tx, matches!(outcome, reference_types::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    pub async fn update_relationship_role(
        &self,
        scope: &Scope,
        id: bilibi_party::RelationshipRoleId,
        update: &relationship_roles::RoleUpdate,
    ) -> std::result::Result<
        reference_types::Outcome<relationship_roles::RelationshipRole>,
        WriteError,
    > {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.relationship_role.update"),
            )
            .await?;
        let outcome = relationship_roles::update(&mut tx, scope, id, update).await?;
        finish(tx, matches!(outcome, reference_types::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    pub async fn retire_relationship_role(
        &self,
        scope: &Scope,
        id: bilibi_party::RelationshipRoleId,
        version: i64,
        retired_on: Option<&bilibi_party::CalendarDate>,
    ) -> std::result::Result<relationship_roles::RelationshipRole, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.relationship_role.retire"),
            )
            .await?;
        let saved = relationship_roles::retire(&mut tx, scope, id, version, retired_on).await?;
        tx.commit().await?;
        Ok(saved)
    }
    pub async fn company_relationships(
        &self,
        scope: &Scope,
        company: bilibi_party::OrganisationId,
        history: bool,
    ) -> Result<Vec<relationships::Entry>> {
        let client = self.db.get().await?;
        relationships::list_for_company(&**client, scope, &self.directory, company, history).await
    }
    pub async fn person_relationships(
        &self,
        scope: &Scope,
        person: bilibi_party::PersonId,
        history: bool,
    ) -> Result<Vec<relationships::Entry>> {
        let client = self.db.get().await?;
        relationships::list_for_person(&**client, scope, &self.directory, person, history).await
    }
    pub async fn relationship(
        &self,
        scope: &Scope,
        id: bilibi_party::RelationshipId,
    ) -> Result<relationships::Entry> {
        let client = self.db.get().await?;
        relationships::read(&**client, scope, &self.directory, id).await
    }
    pub async fn assign_relationship(
        &self,
        scope: &Scope,
        input: &bilibi_party::RelationshipInput,
    ) -> std::result::Result<reference_types::Outcome<relationships::Relationship>, WriteError>
    {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.relationship.assign"),
            )
            .await?;
        let outcome = relationships::assign(&mut tx, scope, &self.proofs, input).await?;
        finish(tx, matches!(outcome, reference_types::Outcome::Saved(_))).await?;
        Ok(outcome)
    }
    pub async fn update_relationship(
        &self,
        scope: &Scope,
        id: bilibi_party::RelationshipId,
        update: &relationships::RelationshipUpdate,
    ) -> std::result::Result<relationships::Relationship, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.relationship.update"),
            )
            .await?;
        let saved = relationships::update(&mut tx, scope, id, update).await?;
        tx.commit().await?;
        Ok(saved)
    }
    pub async fn end_relationship(
        &self,
        scope: &Scope,
        id: bilibi_party::RelationshipId,
        version: i64,
        effective_to: &bilibi_party::CalendarDate,
    ) -> std::result::Result<relationships::Relationship, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.relationship.end"),
            )
            .await?;
        let saved = relationships::end(&mut tx, scope, id, version, effective_to).await?;
        tx.commit().await?;
        Ok(saved)
    }
    pub async fn unlink_relationship(
        &self,
        scope: &Scope,
        id: bilibi_party::RelationshipId,
        version: i64,
    ) -> std::result::Result<relationships::Relationship, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("company.relationship.unlink"),
            )
            .await?;
        let saved = relationships::unlink(&mut tx, scope, id, version).await?;
        tx.commit().await?;
        Ok(saved)
    }
}

// ADR 01 gap, listed so it stays visible: these pages still write through
// the facade rather than an operation contract (`base/agent_api` page).
inventory::submit! { bilibi_module_registry::Contribution::new(bilibi_module_registry::contributions::PAGE_WRITES, "core/company.pages", bilibi_module_registry::Provenance { module: "core/company", source: file!(), line: line!() }, &bilibi_module_registry::contributions::PageWrites {
    service: "core/company.pages",
    contracts: &[],
    pending: bilibi_module_registry::contributions::Pending::Every { reason: "Company edits, lifecycle, activities, departments and reference types have no contract yet; the create form also records activities core.company.create does not take." },
}) }
