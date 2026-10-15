//! Core identities and personal contacts. No HR or implicit account creation.
mod agent_operations;
pub mod contributions;
mod directory;
pub mod identity;
pub mod model;

use bilibi_database::{Db, Error, MutationBoundary, MutationTransaction, Result};
use bilibi_tenancy::{Authentication, Scope};
pub use model::{
    DuplicateQuery, FieldError, Page, Person, PersonId, PersonInput, PersonKind, Search, Update,
};
use std::sync::{Arc, OnceLock};
use tokio::sync::broadcast;

pub const SERVICE: &str = "core/person.persons";
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error(transparent)]
    Backend(#[from] Error),
    #[error("Invalid contact fields")]
    Validation(Vec<FieldError>),
    #[error("This identity is retired")]
    Retired,
    #[error("The platform system identity is protected")]
    SystemIdentity,
}
#[derive(Clone, Debug)]
pub struct PersonChanged {
    pub tenant: i64,
    pub id: PersonId,
}
static CHANGES: OnceLock<broadcast::Sender<PersonChanged>> = OnceLock::new();
pub(crate) fn publish(tx: &mut MutationTransaction<'_>, tenant: i64, id: PersonId) {
    if let Some(changes) = CHANGES.get().cloned() {
        tx.after_commit(move || {
            let _ = changes.send(PersonChanged { tenant, id });
        });
    }
}

pub struct PersonService {
    db: Db,
    mutations: Arc<MutationBoundary>,
    authentication: Arc<Authentication>,
}
impl PersonService {
    pub fn subscribe(&self) -> broadcast::Receiver<PersonChanged> {
        CHANGES
            .get()
            .expect("PersonService installs change sender")
            .subscribe()
    }
    /// Refresh account/tenant eligibility without renewing idle sessions.
    /// HTTP adapters still validate their durable session on every request.
    async fn fresh(
        &self,
        client: &(impl tokio_postgres::GenericClient + Sync),
        scope: &Scope,
    ) -> Result<Scope> {
        let fresh = self
            .authentication
            .authenticate(client, scope.tenant(), scope.user(), scope.csrf.clone())
            .await?;
        Ok(if let Some(agent) = scope.agent() {
            fresh.for_agent(agent.connection(), agent.trace().into())
        } else {
            fresh
        })
    }
    pub async fn read(&self, scope: &Scope, id: PersonId) -> Result<Person> {
        let client = self.db.get().await?;
        let scope = self.fresh(&**client, scope).await?;
        identity::read(&**client, &scope, id).await
    }
    pub async fn search(&self, scope: &Scope, query: &Search) -> Result<Page> {
        let client = self.db.get().await?;
        let scope = self.fresh(&**client, scope).await?;
        identity::search(&**client, &scope, query).await
    }
    pub async fn duplicates(&self, scope: &Scope, query: &DuplicateQuery) -> Result<Vec<Person>> {
        let client = self.db.get().await?;
        let scope = self.fresh(&**client, scope).await?;
        identity::duplicates(&**client, &scope, query).await
    }
    pub async fn create(
        &self,
        scope: &Scope,
        input: &PersonInput,
    ) -> std::result::Result<Person, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let scope = self.fresh(&**client, scope).await?;
        let mut tx = self
            .mutations
            .begin(&mut client, scope.mutation_context("person.create"))
            .await?;
        let person = identity::create(&mut tx, &scope, input).await?;
        tx.commit().await?;
        Ok(person)
    }
    pub async fn update(
        &self,
        scope: &Scope,
        id: PersonId,
        update: &Update,
    ) -> std::result::Result<Person, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let scope = self.fresh(&**client, scope).await?;
        let mut tx = self
            .mutations
            .begin(&mut client, scope.mutation_context("person.update"))
            .await?;
        let person = identity::update(&mut tx, &scope, id, update).await?;
        tx.commit().await?;
        Ok(person)
    }
    pub async fn retire(
        &self,
        scope: &Scope,
        id: PersonId,
        version: i64,
    ) -> std::result::Result<Person, WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let scope = self.fresh(&**client, scope).await?;
        let mut tx = self
            .mutations
            .begin(&mut client, scope.mutation_context("person.retire"))
            .await?;
        let person = identity::retire(&mut tx, &scope, id, version).await?;
        tx.commit().await?;
        Ok(person)
    }
    pub async fn ensure_platform_orchestrator(
        &self,
        scope: &Scope,
    ) -> std::result::Result<(Person, bool), WriteError> {
        let mut client = self.db.get().await.map_err(Error::from)?;
        let scope = self.fresh(&**client, scope).await?;
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("person.system.provision"),
            )
            .await?;
        let result = identity::ensure_platform_orchestrator(&mut tx, &scope).await?;
        if result.1 {
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        Ok(result)
    }
}

inventory::submit! {
    bilibi_module_registry::Module {
        id: "core/person", layer: bilibi_module_registry::Layer::Core,
        dependencies: &["base/database", "base/tenancy", "base/authz", "base/grid", "base/agent_api", "base/principal_directory", "core/party"],
        migrations: &[("001-core-person", include_str!("../migrations/001-person.sql"))],
        services: &[bilibi_module_registry::Service { id: SERVICE, factory: |services| {
            services.get::<bilibi_tenancy::PartyProofs>("base/tenancy.parties")?.bind_persons(Arc::new(identity::Provider))?;
            let directory = services.get::<bilibi_principal_directory::PrincipalDirectory>("base/principal_directory.directory")?;
            directory.bind("core/person", Arc::new(directory::Provider(bilibi_principal_directory::PrincipalKind::Person)))?;
            directory.bind("core/person", Arc::new(directory::Provider(bilibi_principal_directory::PrincipalKind::Agent)))?;
            CHANGES.set(broadcast::channel(128).0).map_err(|_| bilibi_module_registry::RegistryError::Service("duplicate person change sender".into()))?;
            let db = services.get::<Db>("base/database.pool")?.as_ref().clone();
            agent_operations::bind(db.clone())?;
            Ok(Arc::new(PersonService { db, mutations: services.get("base/database.mutations")?, authentication: services.get("base/tenancy.authentication")? }))
        }}],
    }
}
