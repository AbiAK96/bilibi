//! Durable session validation. Background checks never renew activity.
use bilibi_database::{Error, Result};
use bilibi_tenancy::{Authentication, Scope};
use std::sync::Arc;
use tokio_postgres::GenericClient;

/// P2 will supply operator settings. Until then this matches Bilimbi's default.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    idle_seconds: i64,
    touch_seconds: i64,
}
impl Policy {
    pub fn new(idle_seconds: i64, touch_seconds: i64) -> Result<Self> {
        if !(60..=2_592_000).contains(&idle_seconds)
            || touch_seconds < 1
            || touch_seconds > idle_seconds / 2
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            idle_seconds,
            touch_seconds,
        })
    }
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            idle_seconds: 7200,
            touch_seconds: 300,
        }
    }
}
pub struct Sessions {
    authentication: Arc<Authentication>,
    policy: Policy,
}
impl Sessions {
    pub fn new(authentication: Arc<Authentication>, policy: Policy) -> Self {
        Self {
            authentication,
            policy,
        }
    }
    /// Opaque token lookup is the bootstrap exception to tenant scoping. One
    /// lookup checks session expiry, then Tenancy and the owner re-prove identity.
    pub async fn check(&self, client: &(impl GenericClient + Sync), token: &str) -> Result<Scope> {
        self.resolve(client, token, false).await
    }
    /// Call only for authenticated HTTP navigation or accepted user activity.
    /// The conditional update cannot revive an idle or absolutely expired session.
    pub async fn activity(
        &self,
        client: &(impl GenericClient + Sync),
        token: &str,
    ) -> Result<Scope> {
        self.resolve(client, token, true).await
    }
    async fn resolve(
        &self,
        client: &(impl GenericClient + Sync),
        token: &str,
        activity: bool,
    ) -> Result<Scope> {
        if token.is_empty() || token.len() > 256 {
            return Err(Error::Unauthorized);
        }
        let row = client.query_opt("SELECT tenant_id,user_id,csrf FROM sessions WHERE id=$1 AND expires_at>now() AND last_activity>now()-make_interval(secs => $2::double precision)", &[&token, &(self.policy.idle_seconds as f64)]).await?.ok_or(Error::Unauthorized)?;
        let scope = self
            .authentication
            .authenticate(client, row.get(0), row.get(1), row.get(2))
            .await?;
        if activity {
            // Lock the valid row, even when renewal is throttled. The UPDATE
            // executes only when due; a revoked/expired row cannot be revived.
            let touched = client.query_opt("WITH valid AS MATERIALIZED (SELECT id,last_activity FROM sessions WHERE id=$1 AND tenant_id=$2 AND user_id=$3 AND expires_at>now() AND last_activity>now()-make_interval(secs => $4::double precision) FOR UPDATE), touched AS (UPDATE sessions s SET last_activity=now() FROM valid v WHERE s.id=v.id AND v.last_activity<now()-make_interval(secs => $5::double precision) RETURNING s.id) SELECT id FROM valid", &[&token,&scope.tenant(),&scope.user(),&(self.policy.idle_seconds as f64),&(self.policy.touch_seconds as f64)]).await?;
            if touched.is_none() {
                return Err(Error::Unauthorized);
            }
        }
        Ok(scope)
    }
}
inventory::submit! {
    bilibi_module_registry::Module {
        id: "base/session", layer: bilibi_module_registry::Layer::Base,
        dependencies: &["base/database", "base/tenancy"], migrations: &[],
        services: &[bilibi_module_registry::Service { id: "base/session.sessions", factory: |services| Ok(Arc::new(Sessions::new(services.get("base/tenancy.authentication")?, Policy::default()))) }],
    }
}
