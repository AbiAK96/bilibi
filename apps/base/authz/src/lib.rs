//! Authorization decisions and field restrictions, independent of session lifecycle.
use bilibi_database::{Error, Result};
use bilibi_tenancy::Scope;
use std::collections::HashSet;
use tokio_postgres::GenericClient;

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

/// No cached grants. A decision is persisted for every allow and deny.
pub async fn authorize(
    client: &impl GenericClient,
    scope: &Scope,
    capability: Capability,
) -> Result<()> {
    authorize_key(client, scope, capability.key()).await
}

/// String capability seam for contributed vocabulary (P6). No identity or
/// relationship role is itself a grant.
pub async fn authorize_key(client: &impl GenericClient, scope: &Scope, key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 200 {
        return Err(Error::Invalid);
    }
    let roles = client.query("SELECT r.id,r.grant_all FROM principal_roles p JOIN roles r ON r.tenant_id=p.tenant_id AND r.id=p.role_id WHERE p.tenant_id=$1 AND p.user_id=$2", &[&scope.tenant(),&scope.user()]).await?;
    let direct = client.query_opt("SELECT is_allowed FROM principal_capabilities WHERE tenant_id=$1 AND user_id=$2 AND capability=$3", &[&scope.tenant(),&scope.user(),&key]).await?;
    let direct = direct.map(|row| row.get::<_, bool>(0));
    let mut granted = roles.iter().any(|row| row.get::<_, bool>(1));
    if !granted && !roles.is_empty() {
        let ids: Vec<i64> = roles.iter().map(|row| row.get(0)).collect();
        granted = client.query_opt("SELECT capability FROM role_capabilities WHERE tenant_id=$1 AND role_id=ANY($2) AND capability=$3 LIMIT 1", &[&scope.tenant(),&ids,&key]).await?.is_some();
    }
    let allowed = (granted || direct == Some(true)) && direct != Some(false);
    client.execute("INSERT INTO authz_decision_log(tenant_id,user_id,capability,allowed) VALUES($1,$2,$3,$4)", &[&scope.tenant(),&scope.user(),&key,&allowed]).await?;
    if allowed {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

pub async fn restrictions(client: &impl GenericClient, scope: &Scope) -> Result<HashSet<String>> {
    restrictions_for(client, scope, "companies").await
}

/// Every restricting role applies, even when a different role grants all.
pub async fn restrictions_for(
    client: &impl GenericClient,
    scope: &Scope,
    table: &str,
) -> Result<HashSet<String>> {
    if table.is_empty() || table.len() > 200 {
        return Err(Error::Invalid);
    }
    Ok(client.query("SELECT DISTINCT f.field_id FROM field_restrictions f JOIN principal_roles p ON p.tenant_id=f.tenant_id AND p.role_id=f.role_id WHERE f.tenant_id=$1 AND p.user_id=$2 AND f.table_id=$3", &[&scope.tenant(),&scope.user(),&table]).await?.iter().map(|row| row.get(0)).collect())
}

inventory::submit! {
    bilibi_module_registry::Module {
        id:"base/authz",layer:bilibi_module_registry::Layer::Base,
        dependencies:&["base/database","base/tenancy"],migrations:&[],services:&[],
    }
}

/// Operator identity and a capability are independent requirements.
pub async fn authorize_operator(
    client: &(impl GenericClient + Sync),
    scope: &Scope,
    key: &str,
) -> Result<bilibi_tenancy::OperatorProof> {
    let proof = bilibi_tenancy::prove_operator(client, scope).await?;
    authorize_key(client, scope, key).await?;
    Ok(proof)
}
