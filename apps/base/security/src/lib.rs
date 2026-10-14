//! Compatibility facade. New modules use Tenancy, Session and Authz directly.
pub use bilibi_authz::{authorize, restrictions, Capability};
use bilibi_database::Result;
pub use bilibi_session::{Policy, Sessions};
pub use bilibi_tenancy::{
    AccountIdentity, Actor, ActorVerifier, Authentication, ProofClient, ProofFuture, Scope,
};
use std::sync::Arc;
use tokio_postgres::GenericClient;

/// Background-safe compatibility check; activity is a separate explicit call.
pub async fn guard(
    client: &(impl GenericClient + Sync),
    token: &str,
    sessions: &Sessions,
) -> Result<Scope> {
    sessions.check(client, token).await
}
pub struct Security {
    pub db: bilibi_database::Db,
    pub sessions: Arc<Sessions>,
}
impl Security {
    pub async fn guard(&self, token: &str) -> Result<Scope> {
        let client = self.db.get().await?;
        self.sessions.check(&**client, token).await
    }
}
inventory::submit! {
    bilibi_module_registry::Module {
        id:"base/security",layer:bilibi_module_registry::Layer::Base,
        dependencies:&["base/database","base/tenancy","base/session","base/authz"],migrations:&[],
        services:&[bilibi_module_registry::Service {id:"base/security.guard",factory:|services|Ok(Arc::new(Security {db:services.get::<bilibi_database::Db>("base/database.pool")?.as_ref().clone(),sessions:services.get("base/session.sessions")?}))}],
    }
}
