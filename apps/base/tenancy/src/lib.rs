//! Sealed identities for a single unit of work. No Base query of Core tables.
use bilibi_database::{Context, Error, Result};
use bilibi_party_ids::{AffiliationId, OrganisationId, PersonId};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};
use tokio_postgres::{types::ToSql, GenericClient, Row};

pub type ProofFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
/// Object-safe read adapter preserves the caller's transaction for owner proofs.
pub trait ProofClient: Sync {
    fn query_opt<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&'a (dyn ToSql + Sync)],
    ) -> ProofFuture<'a, Option<Row>>;
}
impl<T: GenericClient + Sync> ProofClient for T {
    fn query_opt<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&'a (dyn ToSql + Sync)],
    ) -> ProofFuture<'a, Option<Row>> {
        Box::pin(async move { Ok(GenericClient::query_opt(self, sql, params).await?) })
    }
}

/// Owner output, never deserialize from a browser/job. Core User proves account
/// eligibility; K9 adds the approved party facets through its live owner APIs.
pub struct AccountIdentity {
    pub name: String,
    pub email: String,
    pub person: Option<PersonId>,
    pub organisation: Option<OrganisationId>,
    pub affiliation: Option<AffiliationId>,
}
pub trait ActorVerifier: Send + Sync {
    fn verify<'a>(
        &'a self,
        client: &'a dyn ProofClient,
        tenant: i64,
        user: i64,
    ) -> ProofFuture<'a, AccountIdentity>;
}

#[derive(Clone, Debug)]
pub struct Actor {
    user: i64,
    person: Option<PersonId>,
    organisation: Option<OrganisationId>,
    affiliation: Option<AffiliationId>,
}
impl Actor {
    pub fn user(&self) -> i64 {
        self.user
    }
    pub fn person(&self) -> Option<PersonId> {
        self.person
    }
    pub fn organisation(&self) -> Option<OrganisationId> {
        self.organisation
    }
    pub fn affiliation(&self) -> Option<AffiliationId> {
        self.affiliation
    }
}
/// Private tenant + actor cannot be spliced, constructed, or deserialized by callers.
/// Display fields are compatibility data and never authority.
#[derive(Clone, Debug)]
pub struct Scope {
    tenant: i64,
    actor: Actor,
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
        self.actor.user
    }
    pub fn actor(&self) -> &Actor {
        &self.actor
    }
    pub fn mutation_context(&self, action: impl Into<String>) -> Context {
        Context::user(self.tenant, self.user(), action)
    }
}

#[derive(Default)]
pub struct Authentication {
    verifier: OnceLock<Arc<dyn ActorVerifier>>,
}
impl Authentication {
    /// Trusted boot seam. Duplicate and missing providers fail closed.
    pub fn bind(&self, verifier: Arc<dyn ActorVerifier>) -> bilibi_module_registry::Result<()> {
        self.verifier.set(verifier).map_err(|_| {
            bilibi_module_registry::RegistryError::Service("duplicate actor verifier".into())
        })
    }
    pub fn ready(&self) -> bilibi_module_registry::Result<()> {
        self.verifier.get().map(|_| ()).ok_or_else(|| {
            bilibi_module_registry::RegistryError::Service("missing actor verifier".into())
        })
    }
    /// Authentication edge only: IDs must come from a durable credential/session.
    /// This method establishes identity; it grants no capabilities.
    pub async fn authenticate(
        &self,
        client: &(impl GenericClient + Sync),
        tenant: i64,
        user: i64,
        csrf: String,
    ) -> Result<Scope> {
        if tenant <= 0 || user <= 0 {
            return Err(Error::Unauthorized);
        }
        let row = client
            .query_opt(
                "SELECT name FROM tenants WHERE id=$1 AND deleted_at IS NULL AND status='active'",
                &[&tenant],
            )
            .await?
            .ok_or(Error::Unauthorized)?;
        let identity = self
            .verifier
            .get()
            .ok_or(Error::Unauthorized)?
            .verify(client, tenant, user)
            .await?;
        if identity.affiliation.is_some()
            && (identity.person.is_none() || identity.organisation.is_none())
        {
            return Err(Error::Unauthorized);
        }
        Ok(Scope {
            tenant,
            actor: Actor {
                user,
                person: identity.person,
                organisation: identity.organisation,
                affiliation: identity.affiliation,
            },
            tenant_name: row.get(0),
            user_name: identity.name,
            email: identity.email,
            csrf,
        })
    }
}

/// Explicit persistent operator identity; tenant IDs and grant_all do not imply it.
#[derive(Clone, Debug)]
pub struct OperatorProof {
    scope: Scope,
}
impl OperatorProof {
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
}
pub async fn prove_operator(
    client: &(impl GenericClient + Sync),
    scope: &Scope,
) -> Result<OperatorProof> {
    let row = client.query_opt("SELECT t.id FROM tenants t JOIN platform_operator o ON o.tenant_id=t.id WHERE o.singleton=true AND t.id=$1 AND t.status='active' AND t.deleted_at IS NULL", &[&scope.tenant()]).await?;
    if row.is_none() {
        return Err(Error::Forbidden);
    }
    Ok(OperatorProof {
        scope: scope.clone(),
    })
}

/// Core providers prove live, eligible targets in this exact actor scope. They
/// must reject deleted, archived, cross-tenant and inaccessible identities.
pub trait OrganisationProvider: Send + Sync {
    fn prove<'a>(
        &'a self,
        client: &'a dyn ProofClient,
        scope: &'a Scope,
        id: OrganisationId,
    ) -> ProofFuture<'a, ()>;
}
pub trait PersonProvider: Send + Sync {
    fn prove<'a>(
        &'a self,
        client: &'a dyn ProofClient,
        scope: &'a Scope,
        id: PersonId,
    ) -> ProofFuture<'a, ()>;
}
#[derive(Clone, Debug)]
pub struct OrganisationProof {
    scope: Scope,
    id: OrganisationId,
}
impl OrganisationProof {
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn id(&self) -> OrganisationId {
        self.id
    }
}
#[derive(Clone, Debug)]
pub struct PersonProof {
    scope: Scope,
    id: PersonId,
}
impl PersonProof {
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn id(&self) -> PersonId {
        self.id
    }
}
pub async fn prove_organisation(
    provider: &dyn OrganisationProvider,
    client: &dyn ProofClient,
    scope: &Scope,
    id: OrganisationId,
) -> Result<OrganisationProof> {
    provider.prove(client, scope, id).await?;
    Ok(OrganisationProof {
        scope: scope.clone(),
        id,
    })
}
pub async fn prove_person(
    provider: &dyn PersonProvider,
    client: &dyn ProofClient,
    scope: &Scope,
    id: PersonId,
) -> Result<PersonProof> {
    provider.prove(client, scope, id).await?;
    Ok(PersonProof {
        scope: scope.clone(),
        id,
    })
}

inventory::submit! {
    bilibi_module_registry::Module {
        id: "base/tenancy", layer: bilibi_module_registry::Layer::Base,
        dependencies: &["base/database", "base/party_ids"],
        migrations: &[("005-tenancy-operator", include_str!("../migrations/005-operator.sql"))],
        services: &[bilibi_module_registry::Service { id: "base/tenancy.authentication", factory: |_| Ok(Arc::new(Authentication::default())) }],
    }
}
inventory::submit! {
    bilibi_module_registry::Ready { module: "base/tenancy", check: |services| services.get::<Authentication>("base/tenancy.authentication")?.ready() }
}
