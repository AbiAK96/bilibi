//! Tenant definition service, mirroring Bilimbi `CustomFields` and its owner
//! validation calls. Definitions are data; owners keep record values.
use crate::model::{registered, Definition, FieldType, RecordType, RECORD_TYPES};
use bilibi_authz::{authorize_key, restrictions_for};
use bilibi_database::{Change, Db, Error, MutationBoundary, Result};
use bilibi_module_registry::{Catalog, RegistryError};
use bilibi_tenancy::{Authentication, Scope};
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, str::FromStr, sync::Arc};
use tokio_postgres::GenericClient;

pub const MANAGE: &str = "admin.custom.fields.manage";
const MAX_FIELDS: usize = 100;
const MAX_LABEL: usize = 120;
const MAX_HELP: usize = 500;

#[derive(Clone, Debug)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}
impl FieldError {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FieldInput {
    pub key: String,
    pub label: String,
    pub field_type: FieldType,
    pub required: bool,
    pub help_text: Option<String>,
    pub position: i32,
    pub options: Value,
    pub reference_table: Option<String>,
    pub text_limit: Option<i32>,
    pub format: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Outcome<T> {
    Saved(T),
    Refused(Vec<FieldError>),
    NotFound,
    Stale,
}

pub struct ServiceError;

pub struct CustomFields {
    db: Db,
    boundary: Arc<MutationBoundary>,
    authentication: Arc<Authentication>,
    record_types: Vec<RecordType>,
}
impl CustomFields {
    pub fn new(
        db: Db,
        boundary: Arc<MutationBoundary>,
        authentication: Arc<Authentication>,
    ) -> std::result::Result<Self, RegistryError> {
        let modules = bilibi_module_registry::discover()?;
        let catalog = Catalog::discover(&modules)?;
        let record_types = registered(catalog.entries::<RecordType>(RECORD_TYPES)?)?;
        if record_types.is_empty() {
            return Err(RegistryError::Contribution(
                "base/custom_fields requires at least one opted-in record type".into(),
            ));
        }
        Ok(Self {
            db,
            boundary,
            authentication,
            record_types,
        })
    }

    pub fn record_types(&self) -> &[RecordType] {
        &self.record_types
    }

    /// Whether the current tenant actor may define fields for this opted-in
    /// record type. The page uses this only to hide unavailable write controls;
    /// every definition mutation still rechecks the capability in its transaction.
    pub async fn can_manage_record_type(&self, scope: &Scope, record_type: &str) -> Result<bool> {
        let decl = self.require_record_type(record_type)?;
        let client = self.db.get().await?;
        self.administrator(&**client, scope).await?;
        Ok(bilibi_authz::effective_capabilities(&**client, scope)
            .await?
            .contains(decl.capability))
    }

    pub async fn list(
        &self,
        scope: &Scope,
        record_type: &str,
        archived: bool,
    ) -> Result<Vec<Definition>> {
        let client = self.db.get().await?;
        self.administrator(&**client, scope).await?;
        self.require_record_type(record_type)?;
        definitions(&**client, scope.tenant(), record_type, archived).await
    }

    /// Active definitions for an opted-in record owner's already-authorized
    /// page. Unlike administration listing, this does not require manage
    /// permission; the owner route authorizes the record operation first.
    pub async fn active_for_record(
        &self,
        scope: &Scope,
        record_type: &str,
    ) -> Result<Vec<Definition>> {
        let client = self.db.get().await?;
        self.authentication.reprove(&**client, scope).await?;
        self.require_record_type(record_type)?;
        definitions(&**client, scope.tenant(), record_type, false).await
    }

    pub async fn define(
        &self,
        scope: &Scope,
        record_type: &str,
        input: &FieldInput,
    ) -> Result<Outcome<Definition>> {
        let decl = self.require_record_type(record_type)?;
        let mut client = self.db.get().await?;
        let mut tx = self
            .boundary
            .begin(&mut client, scope.mutation_context("custom_fields.define"))
            .await?;
        self.administrator(tx.sql(), scope).await?;
        authorize_key(tx.sql(), scope, decl.capability).await?;
        if let Some(errors) = validate_definition_input(input) {
            drop(tx);
            return Ok(Outcome::Refused(errors));
        }
        let input = input.clone();
        let record_type = record_type.to_owned();
        let tenant = scope.tenant();
        let result = tx.mutate(move |sql| Box::pin(async move {
            let existing: bool = sql.query_one("SELECT EXISTS(SELECT 1 FROM base_custom_field_definitions WHERE tenant_id=$1 AND record_type=$2 AND field_key=$3)", &[&tenant, &record_type, &input.key]).await?.get(0);
            if existing { return Err(Error::Conflict); }
            let count: i64 = sql.query_one("SELECT count(*) FROM base_custom_field_definitions WHERE tenant_id=$1 AND record_type=$2", &[&tenant, &record_type]).await?.get(0);
            if count >= MAX_FIELDS as i64 { return Err(Error::Invalid); }
            let row = sql.query_one("INSERT INTO base_custom_field_definitions(tenant_id,record_type,field_key,label,field_type,required,help_text,position,options,reference_table,text_limit,format) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING *", &[&tenant,&record_type,&input.key,&input.label,&input.field_type.as_str(),&input.required,&input.help_text,&input.position,&input.options,&input.reference_table,&input.text_limit,&input.format]).await?;
            let def = from_row(&row);
            Ok((def.clone(), vec![Change { kind:"custom_field_definition".into(), id:def.id, old:Value::Null, new:to_json(&def) }]))
        })).await;
        match result {
            Ok(def) => {
                tx.commit().await?;
                Ok(Outcome::Saved(def))
            }
            Err(Error::Conflict) => {
                drop(tx);
                Ok(Outcome::Refused(vec![FieldError::new(
                    "key",
                    "is already in use",
                )]))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn update(
        &self,
        scope: &Scope,
        record_type: &str,
        key: &str,
        version: i64,
        input: &FieldInput,
    ) -> Result<Outcome<Definition>> {
        let decl = self.require_record_type(record_type)?;
        let mut client = self.db.get().await?;
        let mut tx = self
            .boundary
            .begin(&mut client, scope.mutation_context("custom_fields.update"))
            .await?;
        self.administrator(tx.sql(), scope).await?;
        authorize_key(tx.sql(), scope, decl.capability).await?;
        if let Some(errors) = validate_definition_input(input) {
            drop(tx);
            return Ok(Outcome::Refused(errors));
        }
        if key != input.key {
            drop(tx);
            return Ok(Outcome::Refused(vec![FieldError::new(
                "key",
                "cannot be changed",
            )]));
        }
        let record_type = record_type.to_owned();
        let key = key.to_owned();
        let tenant = scope.tenant();
        let input = input.clone();
        let saved = tx.mutate(move |sql| Box::pin(async move {
            let old_row = sql.query_opt("SELECT * FROM base_custom_field_definitions WHERE tenant_id=$1 AND record_type=$2 AND field_key=$3 AND archived_at IS NULL FOR UPDATE", &[&tenant,&record_type,&key]).await?.ok_or(Error::NotFound)?;
            let old = from_row(&old_row);
            if old.version != version { return Err(Error::Conflict); }
            if old.field_type != input.field_type { return Err(Error::Invalid); }
            let row = sql.query_one("UPDATE base_custom_field_definitions SET label=$1,required=$2,help_text=$3,position=$4,options=$5,reference_table=$6,text_limit=$7,format=$8,version=version+1,updated_at=now() WHERE tenant_id=$9 AND record_type=$10 AND field_key=$11 RETURNING *", &[&input.label,&input.required,&input.help_text,&input.position,&input.options,&input.reference_table,&input.text_limit,&input.format,&tenant,&record_type,&key]).await?;
            let new = from_row(&row);
            Ok((new.clone(), vec![Change { kind:"custom_field_definition".into(), id:new.id, old:to_json(&old), new:to_json(&new) }]))
        })).await;
        match saved {
            Ok(saved) => {
                tx.commit().await?;
                Ok(Outcome::Saved(saved))
            }
            Err(Error::NotFound) => {
                drop(tx);
                Ok(Outcome::NotFound)
            }
            Err(Error::Conflict) => {
                drop(tx);
                Ok(Outcome::Stale)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn archive(
        &self,
        scope: &Scope,
        record_type: &str,
        key: &str,
        version: i64,
    ) -> Result<Outcome<Definition>> {
        self.set_archived(scope, record_type, key, version, true)
            .await
    }
    pub async fn restore(
        &self,
        scope: &Scope,
        record_type: &str,
        key: &str,
        version: i64,
    ) -> Result<Outcome<Definition>> {
        self.set_archived(scope, record_type, key, version, false)
            .await
    }
    async fn set_archived(
        &self,
        scope: &Scope,
        record_type: &str,
        key: &str,
        version: i64,
        archive: bool,
    ) -> Result<Outcome<Definition>> {
        let decl = self.require_record_type(record_type)?;
        let action = if archive {
            "custom_fields.archive"
        } else {
            "custom_fields.restore"
        };
        let mut client = self.db.get().await?;
        let mut tx = self
            .boundary
            .begin(&mut client, scope.mutation_context(action))
            .await?;
        self.administrator(tx.sql(), scope).await?;
        authorize_key(tx.sql(), scope, decl.capability).await?;
        let tenant = scope.tenant();
        let rt = record_type.to_owned();
        let key = key.to_owned();
        let saved=tx.mutate(move |sql| Box::pin(async move {
            let old_row=sql.query_opt("SELECT * FROM base_custom_field_definitions WHERE tenant_id=$1 AND record_type=$2 AND field_key=$3 FOR UPDATE", &[&tenant,&rt,&key]).await?.ok_or(Error::NotFound)?;
            let old=from_row(&old_row); if old.version!=version { return Err(Error::Conflict); }
            let row=sql.query_one(if archive {"UPDATE base_custom_field_definitions SET archived_at=now(),version=version+1,updated_at=now() WHERE tenant_id=$1 AND record_type=$2 AND field_key=$3 RETURNING *"} else {"UPDATE base_custom_field_definitions SET archived_at=NULL,version=version+1,updated_at=now() WHERE tenant_id=$1 AND record_type=$2 AND field_key=$3 RETURNING *"}, &[&tenant,&rt,&key]).await?;
            let new=from_row(&row); Ok((new.clone(),vec![Change{kind:"custom_field_definition".into(),id:new.id,old:to_json(&old),new:to_json(&new)}]))
        })).await;
        match saved {
            Ok(saved) => {
                tx.commit().await?;
                Ok(Outcome::Saved(saved))
            }
            Err(Error::NotFound) => {
                drop(tx);
                Ok(Outcome::NotFound)
            }
            Err(Error::Conflict) => {
                drop(tx);
                Ok(Outcome::Stale)
            }
            Err(e) => Err(e),
        }
    }

    /// Validate a custom JSON patch on the owner's mutation transaction. The
    /// owner then writes it into its own row and includes field keys in Change.
    pub async fn validate(
        &self,
        client: &(impl GenericClient + Sync),
        scope: &Scope,
        record_type: &str,
        patch: &Value,
        existing: &Value,
        creating: bool,
    ) -> Result<std::result::Result<Value, Vec<FieldError>>> {
        self.authentication.reprove(client, scope).await?;
        let decl = self.require_record_type(record_type)?;
        let Some(object) = patch.as_object() else {
            return Ok(Err(vec![FieldError::new("custom", "must be an object")]));
        };
        if serde_json::to_vec(patch).map_or(true, |bytes| bytes.len() > 1_048_576) {
            return Ok(Err(vec![FieldError::new(
                "custom",
                "exceeds the size limit",
            )]));
        }
        let current = existing.as_object();
        let defs = definitions(client, scope.tenant(), record_type, false).await?;
        let known: BTreeMap<&str, &Definition> = defs.iter().map(|d| (d.key.as_str(), d)).collect();
        let restricted = restrictions_for(client, scope, decl.grid_table).await?;
        let mut errors = Vec::new();
        let mut out = Map::new();
        for key in object.keys() {
            if !known.contains_key(key.as_str()) {
                errors.push(FieldError::new(
                    format!("custom.{key}"),
                    "is unknown or archived",
                ));
            }
            if restricted.contains(&format!("cf_{key}")) {
                errors.push(FieldError::new(format!("custom.{key}"), "is restricted"));
            }
        }
        for def in &defs {
            let value = object.get(&def.key);
            if value.is_none() {
                if creating && def.required {
                    errors.push(FieldError::new(
                        format!("custom.{}", def.key),
                        "is required",
                    ));
                }
                continue;
            }
            let value = value.unwrap();
            if value.is_null() {
                if def.required {
                    errors.push(FieldError::new(
                        format!("custom.{}", def.key),
                        "is required",
                    ));
                } else {
                    out.insert(def.key.clone(), Value::Null);
                }
                continue;
            }
            if def.required
                && (value.as_str().is_some_and(|text| text.trim().is_empty())
                    || value.as_array().is_some_and(Vec::is_empty))
            {
                errors.push(FieldError::new(
                    format!("custom.{}", def.key),
                    "is required",
                ));
                continue;
            }
            if !valid_value(def, value, current.and_then(|m| m.get(&def.key))) {
                errors.push(FieldError::new(
                    format!("custom.{}", def.key),
                    "has an invalid value",
                ));
            } else {
                out.insert(def.key.clone(), value.clone());
            }
        }
        if !errors.is_empty() {
            return Ok(Err(errors));
        }
        // Patches preserve values that were not submitted.
        let mut merged = current.cloned().unwrap_or_default();
        merged.extend(out);
        if serde_json::to_vec(&merged).map_or(true, |bytes| bytes.len() > 1_048_576) {
            return Ok(Err(vec![FieldError::new(
                "custom",
                "exceeds the size limit",
            )]));
        }
        Ok(Ok(Value::Object(merged)))
    }

    async fn administrator(
        &self,
        client: &(impl GenericClient + Sync),
        scope: &Scope,
    ) -> Result<()> {
        self.authentication.reprove(client, scope).await?;
        authorize_key(client, scope, MANAGE).await
    }
    fn require_record_type(&self, key: &str) -> Result<&RecordType> {
        self.record_types
            .iter()
            .find(|ty| ty.key == key)
            .ok_or(Error::Invalid)
    }
}

fn validate_definition_input(input: &FieldInput) -> Option<Vec<FieldError>> {
    let mut errors = Vec::new();
    if input.key.is_empty()
        || input.key.len() > 63
        || !input
            .key
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase())
        || !input
            .key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        errors.push(FieldError::new(
            "key",
            "must start with a lowercase letter and use lowercase letters, digits or underscores",
        ));
    }
    if input.label.trim().is_empty() || input.label.chars().count() > MAX_LABEL {
        errors.push(FieldError::new(
            "label",
            "is required and must be at most 120 characters",
        ));
    }
    if input
        .help_text
        .as_ref()
        .is_some_and(|s| s.chars().count() > MAX_HELP)
    {
        errors.push(FieldError::new(
            "help_text",
            "must be at most 500 characters",
        ));
    }
    if input.position < 0 {
        errors.push(FieldError::new("position", "must be zero or greater"));
    }
    if input
        .text_limit
        .is_some_and(|n| !(1..=100_000).contains(&n))
    {
        errors.push(FieldError::new(
            "text_limit",
            "must be between 1 and 100000",
        ));
    }
    if input.text_limit.is_some()
        && !matches!(input.field_type, FieldType::Text | FieldType::LongText)
    {
        errors.push(FieldError::new(
            "text_limit",
            "is only available for text fields",
        ));
    }
    if input.format.as_deref().is_some_and(|format| {
        !matches!(format, "email" | "url" | "phone")
            || !matches!(input.field_type, FieldType::Text | FieldType::LongText)
    }) {
        errors.push(FieldError::new(
            "format",
            "is unsupported for this field type",
        ));
    }
    if matches!(input.field_type, FieldType::Choice | FieldType::MultiChoice) {
        if !valid_options(&input.options) {
            errors.push(FieldError::new(
                "options",
                "must contain unique stable codes and labels",
            ));
        }
    } else if input.options.as_array().is_none_or(|a| !a.is_empty()) {
        errors.push(FieldError::new(
            "options",
            "are only available for choice fields",
        ));
    }
    if input.field_type == FieldType::Reference
        && input.reference_table.as_deref().is_none_or(str::is_empty)
    {
        errors.push(FieldError::new(
            "reference_table",
            "is required for a reference field",
        ));
    }
    if input.field_type != FieldType::Reference && input.reference_table.is_some() {
        errors.push(FieldError::new(
            "reference_table",
            "is only available for reference fields",
        ));
    }
    if errors.is_empty() {
        None
    } else {
        Some(errors)
    }
}
fn valid_options(value: &Value) -> bool {
    let Some(items) = value.as_array() else {
        return false;
    };
    if items.is_empty() || items.len() > 500 {
        return false;
    }
    let mut keys = std::collections::HashSet::new();
    items.iter().all(|item| {
        let key = item.get("code").and_then(Value::as_str).unwrap_or_default();
        let label = item
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or_default();
        !key.is_empty()
            && key.len() <= 64
            && key.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.')
            })
            && !label.trim().is_empty()
            && keys.insert(key.to_owned())
    })
}
fn valid_value(def: &Definition, value: &Value, current: Option<&Value>) -> bool {
    match def.field_type {
        FieldType::Text | FieldType::LongText => value.as_str().is_some_and(|s| {
            s.chars().count()
                <= def
                    .text_limit
                    .unwrap_or(if def.field_type == FieldType::LongText {
                        100_000
                    } else {
                        255
                    }) as usize
                && match def.format.as_deref() {
                    Some("email") => s.is_empty() || s.contains('@'),
                    Some("url") => {
                        s.is_empty() || s.starts_with("http://") || s.starts_with("https://")
                    }
                    Some("phone") => s.chars().all(|c| c.is_ascii_digit() || "+() -".contains(c)),
                    _ => true,
                }
        }),
        FieldType::Integer => value.as_i64().is_some(),
        FieldType::Decimal => value
            .as_str()
            .is_some_and(|s| s.parse::<rust_decimal::Decimal>().is_ok()),
        FieldType::Boolean => value.is_boolean(),
        FieldType::Date => value
            .as_str()
            .is_some_and(|s| jiff::civil::Date::from_str(s).is_ok()),
        FieldType::DateTime => value
            .as_str()
            .is_some_and(|s| jiff::Timestamp::from_str(s).is_ok()),
        FieldType::Choice => value.as_str().is_some_and(|code| {
            option_exists(&def.options, code)
                && (option_active(&def.options, code) || current == Some(value))
        }),
        FieldType::MultiChoice => value.as_array().is_some_and(|codes| {
            let mut seen = std::collections::HashSet::new();
            let old = current.and_then(Value::as_array);
            codes.iter().all(|value| {
                value.as_str().is_some_and(|code| {
                    seen.insert(code.to_owned())
                        && (option_active(&def.options, code)
                            || old.is_some_and(|old| {
                                old.iter().any(|prior| prior.as_str() == Some(code))
                            }))
                })
            })
        }),
        FieldType::Reference => value
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 200),
    }
}
fn option_exists(options: &Value, code: &str) -> bool {
    options
        .as_array()
        .is_some_and(|a| a.iter().any(|o| o["code"].as_str() == Some(code)))
}
fn option_active(options: &Value, code: &str) -> bool {
    options.as_array().is_some_and(|a| {
        a.iter()
            .any(|o| o["code"].as_str() == Some(code) && o["active"].as_bool() != Some(false))
    })
}
async fn definitions(
    client: &impl GenericClient,
    tenant: i64,
    record_type: &str,
    archived: bool,
) -> Result<Vec<Definition>> {
    let rows=client.query("SELECT * FROM base_custom_field_definitions WHERE tenant_id=$1 AND record_type=$2 AND ($3 OR archived_at IS NULL) ORDER BY position,field_key", &[&tenant,&record_type,&archived]).await?;
    Ok(rows.iter().map(from_row).collect())
}

pub(crate) async fn grid_fields(
    client: &impl GenericClient,
    scope: &Scope,
) -> Result<Vec<bilibi_grid::RuntimeField>> {
    let mut fields = Vec::new();
    for record_type in &registered_runtime_types()? {
        for def in active_definitions(client, scope.tenant(), record_type.key).await? {
            fields.push(bilibi_grid::RuntimeField {
                table: record_type.grid_table.into(),
                column: record_type.custom_column,
                key: def.key,
                label: def.label,
                ty: grid_type(def.field_type),
                values: def
                    .options
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|option| {
                        option
                            .get("code")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .collect(),
            });
        }
    }
    Ok(fields)
}

pub(crate) async fn authz_fields(
    client: &impl GenericClient,
    scope: &Scope,
) -> Result<Vec<bilibi_authz::field_catalog::CatalogTable>> {
    let mut tables: BTreeMap<String, bilibi_authz::field_catalog::CatalogTable> = BTreeMap::new();
    for record_type in &registered_runtime_types()? {
        let fields = active_definitions(client, scope.tenant(), record_type.key).await?;
        if fields.is_empty() {
            continue;
        }
        let table = tables
            .entry(record_type.grid_table.into())
            .or_insert_with(|| bilibi_authz::field_catalog::CatalogTable {
                id: record_type.grid_table.into(),
                label: record_type.label.into(),
                record_types: vec![record_type.audit_kind.into()],
                fields: Vec::new(),
            });
        table.fields.extend(fields.into_iter().map(|def| {
            bilibi_authz::field_catalog::CatalogField {
                id: format!("cf_{}", def.key),
                label: def.label,
            }
        }));
    }
    Ok(tables.into_values().collect())
}

fn registered_runtime_types() -> Result<Vec<RecordType>> {
    let modules = bilibi_module_registry::discover().map_err(|_| Error::Internal)?;
    let catalog = Catalog::discover(&modules).map_err(|_| Error::Internal)?;
    registered(catalog.entries::<RecordType>(RECORD_TYPES)?).map_err(|_| Error::Internal)
}

fn grid_type(ty: FieldType) -> bilibi_grid::FieldType {
    use bilibi_grid::FieldType as G;
    match ty {
        FieldType::Text | FieldType::LongText | FieldType::Reference | FieldType::MultiChoice => {
            G::String
        }
        FieldType::Integer => G::Integer,
        FieldType::Decimal => G::Decimal,
        FieldType::Date => G::Date,
        FieldType::DateTime => G::DateTime,
        FieldType::Boolean => G::Boolean,
        FieldType::Choice => G::Enum,
    }
}

pub(crate) async fn active_definitions(
    client: &impl GenericClient,
    tenant: i64,
    record_type: &str,
) -> Result<Vec<Definition>> {
    definitions(client, tenant, record_type, false).await
}
fn from_row(row: &tokio_postgres::Row) -> Definition {
    let ty: String = row.get("field_type");
    Definition {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        record_type: row.get("record_type"),
        key: row.get("field_key"),
        label: row.get("label"),
        field_type: FieldType::parse(&ty).unwrap_or(FieldType::Text),
        required: row.get("required"),
        help_text: row.get("help_text"),
        position: row.get("position"),
        options: row.get("options"),
        reference_table: row.get("reference_table"),
        text_limit: row.get("text_limit"),
        format: row.get("format"),
        archived: row
            .get::<_, Option<std::time::SystemTime>>("archived_at")
            .is_some(),
        version: row.get("version"),
    }
}
fn to_json(def: &Definition) -> Value {
    json!({"id":def.id,"tenant_id":def.tenant_id,"record_type":def.record_type,"key":def.key,"label":def.label,"field_type":def.field_type.as_str(),"required":def.required,"options":def.options,"archived":def.archived,"version":def.version})
}
