//! Live authorization, immutable contributed vocabulary, and field withholding.
pub mod capabilities;
mod capabilities_web;
pub mod decision_logs;
pub mod effective;
pub mod field_catalog;
pub mod fields;
pub mod grants;
pub mod restrictions;
mod restrictions_web;
pub mod roles;
mod roles_web;
pub mod vocabulary;
mod web;
use bilibi_database::{Error, Result};
use bilibi_tenancy::Scope;
pub use effective::effective_capabilities;
use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
};
use tokio_postgres::GenericClient;
pub use vocabulary::{
    CapabilityDefinition, RoleDefinition, SystemPrincipalDefinition, Vocabulary,
    VocabularyContribution, VOCABULARY,
};

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
/// Immutable deployment snapshot. Database grants/restrictions are never cached.
/// Never call this from a contribution validator: catalog discovery runs every
/// validator while this snapshot initializes, so the call deadlocks. Prove
/// capability references in a service factory instead.
pub fn vocabulary() -> Result<&'static Vocabulary> {
    static SNAPSHOT: OnceLock<bilibi_module_registry::Result<Vocabulary>> = OnceLock::new();
    SNAPSHOT
        .get_or_init(Vocabulary::from_inventory)
        .as_ref()
        .map_err(|_| Error::Internal)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decision {
    pub allowed: bool,
    pub reason: &'static str,
}
impl Decision {
    fn allow(reason: &'static str) -> Self {
        Self {
            allowed: true,
            reason,
        }
    }
    fn deny(reason: &'static str) -> Self {
        Self {
            allowed: false,
            reason,
        }
    }
    pub fn require(self) -> Result<()> {
        if self.allowed {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }
}
pub async fn authorize(
    client: &impl GenericClient,
    scope: &Scope,
    capability: Capability,
) -> Result<()> {
    authorize_key(client, scope, capability.key()).await
}
pub async fn authorize_key(client: &impl GenericClient, scope: &Scope, key: &str) -> Result<()> {
    decide(client, scope, key).await?.require()
}
/// Unknown keys, invalid live tenants, and non-operator platform requests deny
/// before any grants (including grant_all) can take effect. Denials are logged.
pub async fn decide(client: &impl GenericClient, scope: &Scope, key: &str) -> Result<Decision> {
    if key.is_empty() || key.len() > 200 {
        return Err(Error::Invalid);
    }
    let decision = match vocabulary()?.capabilities.get(key) {
        None => Decision::deny("denied_unknown_capability"),
        Some(cap) => {
            let live = client
                .query_opt(
                    "SELECT id FROM tenants WHERE id=$1 AND status='active' AND deleted_at IS NULL",
                    &[&scope.tenant()],
                )
                .await?
                .is_some();
            let operator = !cap.definition.platform || is_operator(client, scope.tenant()).await?;
            if !live {
                Decision::deny("denied_invalid_actor_context")
            } else if !operator {
                Decision::deny("denied_platform_scope")
            } else {
                let row = client.query_one("SELECT EXISTS(SELECT 1 FROM principal_capabilities WHERE tenant_id=$1 AND user_id=$2 AND capability=$3 AND NOT is_allowed), EXISTS(SELECT 1 FROM principal_capabilities WHERE tenant_id=$1 AND user_id=$2 AND capability=$3 AND is_allowed), EXISTS(SELECT 1 FROM principal_roles p JOIN roles r ON r.tenant_id=p.tenant_id AND r.id=p.role_id WHERE p.tenant_id=$1 AND p.user_id=$2 AND r.grant_all), EXISTS(SELECT 1 FROM principal_roles p JOIN role_capabilities r ON r.tenant_id=p.tenant_id AND r.role_id=p.role_id WHERE p.tenant_id=$1 AND p.user_id=$2 AND r.capability=$3)",&[&scope.tenant(),&scope.user(),&key]).await?;
                if row.get::<_, bool>(0) {
                    Decision::deny("denied_explicitly")
                } else if row.get::<_, bool>(1) {
                    Decision::allow("direct_capability")
                } else if row.get::<_, bool>(2) {
                    Decision::allow("grant_all")
                } else if row.get::<_, bool>(3) {
                    Decision::allow("role_capability")
                } else {
                    Decision::deny("denied_missing_capability")
                }
            }
        }
    };
    log(client, scope.tenant(), scope.user(), None, key, decision).await?;
    Ok(decision)
}
pub(crate) async fn is_operator(client: &impl GenericClient, tenant: i64) -> Result<bool> {
    Ok(client.query_opt("SELECT o.tenant_id FROM platform_operator o JOIN tenants t ON t.id=o.tenant_id WHERE o.singleton AND o.tenant_id=$1 AND t.status='active' AND t.deleted_at IS NULL",&[&tenant]).await?.is_some())
}
async fn log(
    client: &impl GenericClient,
    tenant: i64,
    user: i64,
    principal: Option<&str>,
    key: &str,
    decision: Decision,
) -> Result<()> {
    let kind = if user == 0 { "system" } else { "user" };
    client.execute("INSERT INTO authz_decision_log(tenant_id,user_id,capability,allowed,reason,principal_type,system_principal) VALUES($1,$2,$3,$4,$5,$6,$7)",&[&tenant,&user,&key,&decision.allowed,&decision.reason,&kind,&principal]).await?;
    Ok(())
}
/// Trusted worker identity; not deserializable or usable as a user scope. It
/// holds no roles, cannot administer grants, and gains nothing by declaration.
#[derive(Clone, Debug)]
pub struct SystemScope {
    tenant: i64,
    principal: Option<String>,
}
impl SystemScope {
    pub fn tenant(&self) -> i64 {
        self.tenant
    }
    pub fn principal(&self) -> Option<&str> {
        self.principal.as_deref()
    }
    /// Attribution for an already authorized worker write. Context is evidence,
    /// never a capability grant; the worker must authorize before beginning it.
    pub fn mutation_context(&self, action: impl Into<String>) -> bilibi_database::Context {
        let mut context = bilibi_database::Context::user(self.tenant, 0, action);
        context.actor = bilibi_database::Principal {
            kind: "system".into(),
            id: self.principal.clone().unwrap_or_else(|| "0".into()),
        };
        context
    }
    /// Rehydrate persisted worker metadata at the trusted execution edge. Every
    /// decision still checks the live tenant, declaration and durable grant.
    pub async fn rehydrate(
        client: &impl GenericClient,
        tenant: i64,
        principal: Option<&str>,
    ) -> Result<Self> {
        if client
            .query_opt(
                "SELECT id FROM tenants WHERE id=$1 AND status='active' AND deleted_at IS NULL",
                &[&tenant],
            )
            .await?
            .is_none()
        {
            return Err(Error::Unauthorized);
        }
        if principal
            .is_some_and(|name| !vocabulary().is_ok_and(|v| v.system_principals.contains_key(name)))
        {
            return Err(Error::Forbidden);
        }
        Ok(Self {
            tenant,
            principal: principal.map(str::to_owned),
        })
    }
}
pub async fn decide_system(
    client: &impl GenericClient,
    scope: &SystemScope,
    key: &str,
) -> Result<Decision> {
    if key.is_empty() || key.len() > 200 {
        return Err(Error::Invalid);
    }
    let vocab = vocabulary()?;
    let live = client
        .query_opt(
            "SELECT id FROM tenants WHERE id=$1 AND status='active' AND deleted_at IS NULL",
            &[&scope.tenant],
        )
        .await?
        .is_some();
    let d = if !live {
        Decision::deny("denied_invalid_actor_context")
    } else if !vocab.capabilities.contains_key(key) {
        Decision::deny("denied_unknown_capability")
    } else if scope.principal.is_none() {
        Decision::deny("denied_no_principal")
    } else if vocab.capabilities[key].definition.platform
        && !is_operator(client, scope.tenant).await?
    {
        Decision::deny("denied_platform_scope")
    } else if !vocab
        .system_principals
        .get(scope.principal().unwrap())
        .is_some_and(|p| p.capabilities.contains(&key))
    {
        Decision::deny("denied_undeclared_capability")
    } else {
        match client.query_opt("SELECT is_allowed FROM authz_system_grants WHERE tenant_id=$1 AND principal=$2 AND capability=$3",&[&scope.tenant,&scope.principal(),&key]).await?.map(|r|r.get::<_,bool>(0)) {
            Some(true)=>Decision::allow("system_principal_grant"), Some(false)=>Decision::deny("denied_explicitly"),None=>Decision::deny("denied_missing_capability"),
        }
    };
    log(client, scope.tenant, 0, scope.principal(), key, d).await?;
    Ok(d)
}
pub async fn authorize_system(
    client: &impl GenericClient,
    scope: &SystemScope,
    key: &str,
) -> Result<()> {
    decide_system(client, scope, key).await?.require()
}
/// Operator identity and capability are independent, logged requirements.
pub async fn authorize_operator(
    client: &(impl GenericClient + Sync),
    scope: &Scope,
    key: &str,
) -> Result<bilibi_tenancy::OperatorProof> {
    if !is_operator(client, scope.tenant()).await? {
        if key.is_empty() || key.len() > 200 {
            return Err(Error::Invalid);
        }
        log(
            client,
            scope.tenant(),
            scope.user(),
            None,
            key,
            Decision::deny("denied_platform_scope"),
        )
        .await?;
        return Err(Error::Forbidden);
    }
    authorize_key(client, scope, key).await?;
    bilibi_tenancy::prove_operator(client, scope).await
}
/// Whether `key` is granted by name, directly or through a role listing it,
/// with no direct denial. A `grant_all` role does not count: some capabilities
/// (stored-secret reveal) must be handed out deliberately. Pair it with
/// `decide`, which still applies tenant, platform and logging rules.
pub async fn explicitly_granted(
    client: &impl GenericClient,
    scope: &Scope,
    key: &str,
) -> Result<bool> {
    Ok(client.query_one("SELECT NOT EXISTS(SELECT 1 FROM principal_capabilities WHERE tenant_id=$1 AND user_id=$2 AND capability=$3 AND NOT is_allowed) AND (EXISTS(SELECT 1 FROM principal_capabilities WHERE tenant_id=$1 AND user_id=$2 AND capability=$3 AND is_allowed) OR EXISTS(SELECT 1 FROM principal_roles p JOIN role_capabilities r ON r.tenant_id=p.tenant_id AND r.role_id=p.role_id WHERE p.tenant_id=$1 AND p.user_id=$2 AND r.capability=$3))",&[&scope.tenant(),&scope.user(),&key]).await?.get(0))
}
pub async fn restrictions(client: &impl GenericClient, scope: &Scope) -> Result<HashSet<String>> {
    restrictions_for(client, scope, "companies").await
}
pub async fn restrictions_for(
    client: &impl GenericClient,
    scope: &Scope,
    table: &str,
) -> Result<HashSet<String>> {
    if table.is_empty() || table.len() > 200 {
        return Err(Error::Invalid);
    }
    Ok(client.query("SELECT DISTINCT f.field_id FROM field_restrictions f JOIN principal_roles p ON p.tenant_id=f.tenant_id AND p.role_id=f.role_id WHERE f.tenant_id=$1 AND p.user_id=$2 AND f.table_id=$3",&[&scope.tenant(),&scope.user(),&table]).await?.iter().map(|row|row.get(0)).collect())
}
inventory::submit! { bilibi_module_registry::Module {
    id:"base/authz",layer:bilibi_module_registry::Layer::Base,
    dependencies:&["base/module_registry","base/database","base/tenancy","base/audit","base/menu","base/settings","base/ui","base/locale","base/datetime"],
    migrations:&[("007-authz-policy",include_str!("../migrations/007-policy.sql")),("base-authz-008-decision-log-restrictions",include_str!("../migrations/008-decision-log-restrictions.sql")),("base-authz-009-roles-admin",include_str!("../migrations/009-roles-admin.sql"))],
    services:&[bilibi_module_registry::Service {id:"base/authz.grants",factory:|services| Ok(Arc::new(grants::Grants::new(
        services.get::<bilibi_database::Db>("base/database.pool")?.as_ref().clone(),
        services.get("base/database.mutations")?, vocabulary::Vocabulary::from_inventory()?, services.get("base/tenancy.authentication")?,
    ))) }, bilibi_module_registry::Service { id:"base/authz.seeds", factory:|services| Ok(Arc::new(bilibi_database::seeds::SeedService(services.get::<grants::Grants>("base/authz.grants")?))) },
    bilibi_module_registry::Service {id:"base/authz.field_restrictions",factory:|services| Ok(Arc::new(restrictions::FieldRestrictions::new(
        services.get::<bilibi_database::Db>("base/database.pool")?.as_ref().clone(),
        services.get("base/database.mutations")?, services.get("base/tenancy.authentication")?,
    ))) },
    bilibi_module_registry::Service {id:"base/authz.pages",factory:|services| Ok(Arc::new(bilibi_ui::page::PageService::<Scope, Error>(Arc::new(web::Pages {
        db: services.get::<bilibi_database::Db>("base/database.pool")?.as_ref().clone(),
        display: services.get("base/datetime.display")?, locales: services.get("base/locale.locales")?,
        grants: services.get("base/authz.grants")?,
        restrictions: services.get("base/authz.field_restrictions")?,
    })))) }],
} }

inventory::submit! {
    bilibi_module_registry::Contribution::new(bilibi_module_registry::contributions::ROUTES, "base/authz.decision-logs",
        bilibi_module_registry::Provenance {module:"base/authz",source:file!(),line:line!()},
        &bilibi_module_registry::contributions::Route { method:bilibi_module_registry::contributions::Method::Get, path:"/authz/decision-logs",
            service:"base/authz.pages", operation:"decision-logs", access:bilibi_module_registry::contributions::Access::Capability(decision_logs::LIST), framed:true })
}
macro_rules! page_route {
    ($id:literal, $path:literal, $operation:literal, $capability:expr) => {
        inventory::submit! {
            bilibi_module_registry::Contribution::new(bilibi_module_registry::contributions::ROUTES, $id,
                bilibi_module_registry::Provenance {module:"base/authz",source:file!(),line:line!()},
                &bilibi_module_registry::contributions::Route { method:bilibi_module_registry::contributions::Method::Get, path:$path,
                    service:"base/authz.pages", operation:$operation, access:bilibi_module_registry::contributions::Access::Capability($capability), framed:true })
        }
    };
}
page_route!(
    "base/authz.capabilities",
    "/authz/capabilities",
    "capabilities",
    capabilities::LIST
);
page_route!("base/authz.roles", "/authz/roles", "roles", roles::LIST);
// A literal segment outranks the parameter, so "create" never reaches the
// detail page as an id.
page_route!(
    "base/authz.role-create",
    "/authz/roles/create",
    "role-create",
    roles::CREATE
);
page_route!("base/authz.role", "/authz/roles/{id}", "role", roles::VIEW);
page_route!(
    "base/authz.field-restrictions",
    "/authz/field-restrictions",
    "field-restrictions",
    restrictions::MANAGE
);
// Bilimbi's Authorization menu (`contributions.ex`): items join it as their
// screens land; the container stays hidden until one is visible.
bilibi_menu::contribute!(
    "base/authz",
    bilibi_menu::MenuItem::container("admin.authz", "Authorization")
        .icon("shield-check")
        .under("admin")
        .order(20)
);
bilibi_menu::contribute!(
    "base/authz",
    bilibi_menu::MenuItem::page(
        "admin.authz.capability",
        "Capabilities",
        "/authz/capabilities"
    )
    .icon("puzzle-piece")
    .under("admin.authz")
    .requires(bilibi_menu::Requirement::Capability(capabilities::LIST))
    .order(10)
);
bilibi_menu::contribute!(
    "base/authz",
    bilibi_menu::MenuItem::page("admin.authz.role", "Roles", "/authz/roles")
        .icon("shield-check")
        .under("admin.authz")
        .requires(bilibi_menu::Requirement::Capability(roles::LIST))
        .order(20)
);
bilibi_menu::contribute!(
    "base/authz",
    bilibi_menu::MenuItem::page(
        "admin.authz.decision-log",
        "Decision Logs",
        "/authz/decision-logs"
    )
    .icon("clipboard-document-list")
    .under("admin.authz")
    .requires(bilibi_menu::Requirement::Capability(decision_logs::LIST))
    .order(50)
);
bilibi_menu::contribute!(
    "base/authz",
    bilibi_menu::MenuItem::page(
        "admin.authz.field-restrictions",
        "Field Restrictions",
        "/authz/field-restrictions"
    )
    .icon("lock-closed")
    .under("admin.authz")
    .requires(bilibi_menu::Requirement::Capability(restrictions::MANAGE))
    .order(60)
);

inventory::submit! {
    bilibi_module_registry::Contribution::new(bilibi_module_registry::contributions::SEEDS,
        "authz.roles.v1", bilibi_module_registry::Provenance {module:"base/authz",source:file!(),line:line!()},
        &bilibi_module_registry::contributions::Seed { version:"authz-system-roles-v1", mode:bilibi_module_registry::contributions::SeedMode::Production,
            action:bilibi_module_registry::contributions::SeedAction::Service {service:"base/authz.seeds",operation:"reconcile"} })
}
