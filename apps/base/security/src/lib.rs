//! Fresh session/scope checks, deny-wins capabilities, and any-role field masking.
use bilibi_database::{Error, Result};
use std::collections::HashSet;
use tokio_postgres::GenericClient;

#[derive(Clone, Debug)]
pub struct Scope {
    tenant: i64,
    user: i64,
    pub tenant_name: String,
    pub user_name: String,
    pub email: String,
    pub csrf: String,
}
impl Scope {
    pub fn tenant(&self) -> i64 {
        self.tenant
    }
    pub fn user(&self) -> i64 {
        self.user
    }
}

#[derive(Clone, Copy)]
pub enum Capability {
    CompaniesView,
    CompaniesUpdate,
    NotificationsView,
    NotificationsUpdate,
    SessionLogout,
}
impl Capability {
    pub fn key(self) -> &'static str {
        match self {
            Self::CompaniesView => "companies.view",
            Self::CompaniesUpdate => "companies.update",
            Self::NotificationsView => "notifications.view",
            Self::NotificationsUpdate => "notifications.update",
            Self::SessionLogout => "session.logout",
        }
    }
}

/// Session token lookup is the bootstrap exception to tenant scoping: its row
/// supplies the trusted tenant. Nothing from a route/event can select a tenant.
pub async fn guard(client: &impl GenericClient, token: &str) -> Result<Scope> {
    let row = client
        .query_opt(
            "SELECT tenant_id,user_id,csrf FROM sessions WHERE id=$1 AND expires_at>now()",
            &[&token],
        )
        .await?
        .ok_or(Error::Unauthorized)?;
    let tenant: i64 = row.get(0);
    let user: i64 = row.get(1);
    let csrf = row.get(2);
    let tenant_row = client
        .query_opt(
            "SELECT name FROM tenants WHERE id=$1 AND deleted_at IS NULL AND status='active'",
            &[&tenant],
        )
        .await?
        .ok_or(Error::Unauthorized)?;
    let user_row = client.query_opt("SELECT name,email FROM users WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL AND status='active'", &[&tenant,&user]).await?.ok_or(Error::Unauthorized)?;
    client.execute("UPDATE sessions SET last_activity=now() WHERE tenant_id=$1 AND id=$2 AND expires_at>now() AND last_activity<now()-interval '60 seconds'", &[&tenant,&token]).await?;
    Ok(Scope {
        tenant,
        user,
        tenant_name: tenant_row.get(0),
        user_name: user_row.get(0),
        email: user_row.get(1),
        csrf,
    })
}

/// No cached grants. A decision is persisted for every allow and deny.
pub async fn authorize(
    client: &impl GenericClient,
    scope: &Scope,
    capability: Capability,
) -> Result<()> {
    let key = capability.key();
    let roles = client.query("SELECT r.id,r.grant_all FROM principal_roles p JOIN roles r ON r.tenant_id=p.tenant_id AND r.id=p.role_id WHERE p.tenant_id=$1 AND p.user_id=$2", &[&scope.tenant,&scope.user]).await?;
    let direct = client.query_opt("SELECT is_allowed FROM principal_capabilities WHERE tenant_id=$1 AND user_id=$2 AND capability=$3", &[&scope.tenant,&scope.user,&key]).await?;
    let direct = direct.map(|row| row.get::<_, bool>(0));
    let mut granted = roles.iter().any(|row| row.get::<_, bool>(1));
    if !granted && !roles.is_empty() {
        let ids: Vec<i64> = roles.iter().map(|row| row.get(0)).collect();
        granted = client.query_opt("SELECT capability FROM role_capabilities WHERE tenant_id=$1 AND role_id=ANY($2) AND capability=$3 LIMIT 1", &[&scope.tenant,&ids,&key]).await?.is_some();
    }
    let allowed = (granted || direct == Some(true)) && direct != Some(false);
    client.execute("INSERT INTO authz_decision_log(tenant_id,user_id,capability,allowed) VALUES($1,$2,$3,$4)", &[&scope.tenant,&scope.user,&key,&allowed]).await?;
    if allowed {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

pub async fn restrictions(client: &impl GenericClient, scope: &Scope) -> Result<HashSet<String>> {
    Ok(client.query("SELECT DISTINCT f.field_id FROM field_restrictions f JOIN principal_roles p ON p.tenant_id=f.tenant_id AND p.role_id=f.role_id WHERE f.tenant_id=$1 AND p.user_id=$2 AND f.table_id='companies'", &[&scope.tenant,&scope.user]).await?.iter().map(|row| row.get(0)).collect())
}

/// Named service resolved by the host; static functions also work within a caller's transaction.
pub struct Security {
    pub db: bilibi_database::Db,
}
impl Security {
    pub async fn guard(&self, token: &str) -> Result<Scope> {
        let client = self.db.get().await?;
        guard(&**client, token).await
    }
}
inventory::submit! {
    bilibi_module_registry::Module {
        id:"base/security",layer:bilibi_module_registry::Layer::Base,
        dependencies:&["base/database"],migrations:&[],
        services:&[bilibi_module_registry::Service {id:"base/security.guard",factory:|services|Ok(std::sync::Arc::new(Security {db:services.get::<bilibi_database::Db>("base/database.pool")?.as_ref().clone()}))}],
    }
}
