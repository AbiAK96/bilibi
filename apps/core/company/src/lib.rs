//! Tenant-scoped company reads and atomic, audited inline edits.
mod contributions;

use bilibi_database::{Change, Db, Error, MutationBoundary, Result};
use bilibi_notifications::NotificationBoundary;
use bilibi_security::{authorize, guard, restrictions, Capability, Scope, Sessions};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use tokio_postgres::{GenericClient, Row};

pub const MIGRATION: (&str, &str) = ("003-company", include_str!("../migrations/003.sql"));
pub const MASK: &str = "••••";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Params {
    pub page: i64,
    pub size: i64,
    pub sort: String,
    pub dir: String,
    pub q: String,
    pub status: String,
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
        }
    }
}
impl Params {
    pub fn validate(&self) -> Result<()> {
        if !(1..=100_000).contains(&self.page)
            || ![25, 50, 100].contains(&self.size)
            || !["name", "status", "created_at"].contains(&self.sort.as_str())
            || !["asc", "desc"].contains(&self.dir.as_str())
            || self.q.chars().count() > 64
            || !["", "active", "suspended", "pending", "archived"].contains(&self.status.as_str())
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Company {
    pub id: i64,
    pub name: String,
    pub status: String,
    pub email: String,
    pub phone: String,
    pub tax_id: String,
    pub revenue: String,
    pub version: i64,
}
impl Company {
    /// Organisation identity of this company, preserving existing record IDs.
    /// The ID alone does not prove tenant ownership or authorization.
    pub fn organisation_id(&self) -> Result<bilibi_party_ids::OrganisationId> {
        bilibi_party_ids::OrganisationId::new(self.id).map_err(|_| Error::Invalid)
    }

    fn from_row(row: &Row, masked: &HashSet<String>) -> Self {
        let read = |field: &str| {
            if masked.contains(field) {
                MASK.to_owned()
            } else {
                row.get(field)
            }
        };
        Self {
            id: row.get("id"),
            name: row.get("name"),
            status: row.get("status"),
            email: read("email"),
            phone: read("phone"),
            tax_id: read("tax_id"),
            revenue: read("revenue"),
            version: row.get("version"),
        }
    }
}
pub struct Page {
    pub rows: Vec<Company>,
    pub total: i64,
}
const COLUMNS: &str = "id,name,status,email,phone,tax_id,revenue::text AS revenue,version";

pub async fn list(client: &impl GenericClient, scope: &Scope, params: &Params) -> Result<Page> {
    params.validate()?;
    authorize(client, scope, Capability::CompaniesView).await?;
    let masked = restrictions(client, scope).await?;
    // Escape LIKE metacharacters: search is a literal, case-insensitive substring.
    let query = format!(
        "%{}%",
        params
            .q
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let filter = "tenant_id=$1 AND deleted_at IS NULL AND ($2='' OR status=$2) AND name ILIKE $3";
    let args: &[&(dyn tokio_postgres::types::ToSql + Sync)] =
        &[&scope.tenant(), &params.status, &query];
    let total = client
        .query_one(
            &format!("SELECT count(*) FROM companies WHERE {filter}"),
            args,
        )
        .await?
        .get(0);
    // Only validated enum values reach the two SQL identifiers below.
    let sql = format!(
        "SELECT {COLUMNS} FROM companies WHERE {filter} ORDER BY {} {}, id {} LIMIT $4 OFFSET $5",
        params.sort, params.dir, params.dir
    );
    let offset = (params.page - 1) * params.size;
    let rows = client
        .query(
            &sql,
            &[
                &scope.tenant(),
                &params.status,
                &query,
                &params.size,
                &offset,
            ],
        )
        .await?
        .iter()
        .map(|row| Company::from_row(row, &masked))
        .collect();
    Ok(Page { rows, total })
}

pub async fn detail(
    client: &impl GenericClient,
    scope: &Scope,
    id: i64,
) -> Result<(Company, HashSet<String>)> {
    authorize(client, scope, Capability::CompaniesView).await?;
    let masked = restrictions(client, scope).await?;
    let row = client.query_opt(&format!("SELECT {COLUMNS} FROM companies WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL"), &[&scope.tenant(),&id]).await?.ok_or(Error::NotFound)?;
    Ok((Company::from_row(&row, &masked), masked))
}

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
        let valid = match self.field.as_str() {
            "name" => !self.value.trim().is_empty() && self.value.chars().count() <= 160,
            "status" => {
                ["active", "suspended", "pending", "archived"].contains(&self.value.as_str())
            }
            "email" => {
                self.value.len() <= 254 && (self.value.is_empty() || self.value.contains('@'))
            }
            "phone" | "tax_id" => self.value.chars().count() <= 64,
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
) -> Result<()> {
    edit.validate()?;
    let mut client = db.get().await?;
    let scope = guard(&**client, token, sessions).await?;
    let mut tx = mutations
        .begin(&mut client, scope.mutation_context("company.update"))
        .await?;
    let scope = guard(tx.sql(), token, sessions).await?;
    authorize(tx.sql(), &scope, Capability::CompaniesUpdate).await?;
    if restrictions(tx.sql(), &scope).await?.contains(&edit.field) {
        return Err(Error::Forbidden);
    }
    let id = edit.id;
    let version = edit.version;
    let field = edit.field.clone();
    let value = edit.value.clone();
    let tenant = scope.tenant();
    tx.mutate(move |sql| Box::pin(async move {
        let row = sql.query_opt(&format!("SELECT {},version,status FROM companies WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE",field), &[&tenant,&id]).await?.ok_or(Error::NotFound)?;
        if row.get::<_, i64>(1) != version { return Err(Error::Conflict); }
        if row.get::<_, String>(2) == "archived" { return Err(Error::Forbidden); }
        let (old, new) = if field == "revenue" {
            let value = value.parse::<i64>().map_err(|_| Error::Invalid)?;
            sql.execute("UPDATE companies SET revenue=$1,version=version+1 WHERE tenant_id=$2 AND id=$3", &[&value,&tenant,&id]).await?;
            (json!(row.get::<_, i64>(0)), json!(value))
        } else {
            sql.execute(&format!("UPDATE companies SET {}=$1,version=version+1 WHERE tenant_id=$2 AND id=$3",field), &[&value,&tenant,&id]).await?;
            (json!(row.get::<_, String>(0)), json!(value))
        };
        Ok(((), vec![Change { kind: "company".into(), id, old: json!({&field:old}), new: json!({&field:new}) }]))
    })).await?;
    notifications
        .record(&mut tx, &scope, "Company updated")
        .await?;
    tx.commit().await
}

pub mod web;

pub struct CompanyService {
    sessions: std::sync::Arc<Sessions>,
    db: Db,
    notifications: std::sync::Arc<NotificationBoundary>,
    mutations: std::sync::Arc<MutationBoundary>,
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
        )
        .await
    }
}
inventory::submit! {
    bilibi_module_registry::Module {
        id:"core/company",layer:bilibi_module_registry::Layer::Core,
        dependencies:&["base/database","base/security","base/ui","base/notifications","base/party_ids","base/session","base/menu"],migrations:&[MIGRATION],
        services:&[bilibi_module_registry::Service {id:"core/company.pages",factory:|services|Ok(std::sync::Arc::new(bilibi_ui::page::PageService(std::sync::Arc::new(web::Pages { db: services.get::<Db>("base/database.pool")?.as_ref().clone() }))))}, bilibi_module_registry::Service {id:"core/company.companies",factory:|services|Ok(std::sync::Arc::new(CompanyService {sessions:services.get("base/session.sessions")?,mutations:services.get("base/database.mutations")?,db:services.get::<Db>("base/database.pool")?.as_ref().clone(),notifications:services.get::<NotificationBoundary>("base/notifications.sink")?}))}],
    }
}
