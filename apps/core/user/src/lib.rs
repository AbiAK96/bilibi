//! Credentials, server sessions, and the user's notification bell.
use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use bilibi_database::{audit, Db, Error, Mutation, Result};
use bilibi_security::{authorize, guard, Capability, Scope};
use serde_json::json;
use tokio::sync::broadcast;
use tokio_postgres::GenericClient;

pub const MIGRATION: (&str, &str) = ("002-user", include_str!("../migrations/002.sql"));

pub fn hash_password(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|_| Error::Internal)
}

pub struct Login {
    pub token: String,
    pub csrf: String,
}

/// Tenant+email is the explicit credential lookup, not an unscoped user search.
pub async fn login(db: &Db, tenant: i64, email: &str, password: &str) -> Result<Login> {
    if tenant <= 0 || email.len() > 254 || password.len() > 1024 {
        return Err(Error::Invalid);
    }
    let client = db.get().await?;
    let row = client.query_opt("SELECT u.id,u.password_hash FROM users u JOIN tenants t ON t.id=u.tenant_id WHERE u.tenant_id=$1 AND u.email=$2 AND u.deleted_at IS NULL AND u.status='active' AND t.deleted_at IS NULL AND t.status='active'", &[&tenant,&email]).await?;
    // Perform a real Argon2 verification for unknown accounts as well.
    let dummy = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRmb3J0ZXN0aW5n$0A3fpxjnP4JPN0QSxhKQJNfBQYkxMvEQvEhu0Zp5H8c";
    let hash: String = row
        .as_ref()
        .map(|r| r.get(1))
        .unwrap_or_else(|| dummy.to_owned());
    let password = password.to_owned();
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).ok().is_some_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .map_err(|_| Error::Internal)?;
    if !valid {
        return Err(Error::Unauthorized);
    }
    let user: i64 = row.ok_or(Error::Unauthorized)?.get(0);
    let result = Login {
        token: uuid::Uuid::new_v4().simple().to_string(),
        csrf: uuid::Uuid::new_v4().simple().to_string(),
    };
    client.execute("INSERT INTO sessions(id,tenant_id,user_id,csrf,expires_at) VALUES($1,$2,$3,$4,now()+interval '24 hours')", &[&result.token,&tenant,&user,&result.csrf]).await?;
    Ok(result)
}

#[derive(Clone, Debug)]
pub struct BellEvent {
    pub tenant: i64,
    pub user: i64,
}
#[derive(Clone)]
pub struct Notifications {
    tx: broadcast::Sender<BellEvent>,
}
impl Default for Notifications {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(128);
        Self { tx }
    }
}
impl Notifications {
    pub fn subscribe(&self) -> broadcast::Receiver<BellEvent> {
        self.tx.subscribe()
    }
    pub fn changed(&self, scope: &Scope) {
        let _ = self.tx.send(BellEvent {
            tenant: scope.tenant(),
            user: scope.user(),
        });
    }
}

pub async fn unread(client: &impl GenericClient, scope: &Scope) -> Result<i64> {
    authorize(client, scope, Capability::NotificationsView).await?;
    Ok(client.query_one("SELECT count(*) FROM notifications WHERE tenant_id=$1 AND user_id=$2 AND read_at IS NULL", &[&scope.tenant(),&scope.user()]).await?.get(0))
}

/// Used by a business transaction; the bell is published only after commit.
pub async fn saved_notification(tx: &impl GenericClient, scope: &Scope) -> Result<()> {
    let row = tx.query_one("INSERT INTO notifications(tenant_id,user_id,message) VALUES($1,$2,'Company updated') RETURNING id", &[&scope.tenant(),&scope.user()]).await?;
    audit(
        tx,
        Mutation {
            tenant: scope.tenant(),
            actor: scope.user(),
            kind: "notification",
            id: row.get(0),
            old: json!({}),
            new: json!({"message":"Company updated"}),
        },
    )
    .await
}

pub async fn mark_read(db: &Db, token: &str, notifications: &Notifications) -> Result<()> {
    let mut client = db.get().await?;
    let scope = guard(&**client, token).await?;
    authorize(&**client, &scope, Capability::NotificationsUpdate).await?;
    let tx = client.transaction().await?;
    let rows = tx.query("SELECT id FROM notifications WHERE tenant_id=$1 AND user_id=$2 AND read_at IS NULL FOR UPDATE", &[&scope.tenant(),&scope.user()]).await?;
    for row in rows {
        let id: i64 = row.get(0);
        tx.execute(
            "UPDATE notifications SET read_at=now() WHERE tenant_id=$1 AND user_id=$2 AND id=$3",
            &[&scope.tenant(), &scope.user(), &id],
        )
        .await?;
        audit(
            &*tx,
            Mutation {
                tenant: scope.tenant(),
                actor: scope.user(),
                kind: "notification",
                id,
                old: json!({"read":false}),
                new: json!({"read":true}),
            },
        )
        .await?;
    }
    tx.commit().await?;
    notifications.changed(&scope);
    Ok(())
}

pub async fn logout(db: &Db, token: &str, csrf: &str) -> Result<()> {
    let client = db.get().await?;
    let scope = guard(&**client, token).await?;
    if scope.csrf != csrf {
        return Err(Error::Forbidden);
    }
    authorize(&**client, &scope, Capability::SessionLogout).await?;
    client
        .execute(
            "DELETE FROM sessions WHERE tenant_id=$1 AND user_id=$2 AND id=$3",
            &[&scope.tenant(), &scope.user(), &token],
        )
        .await?;
    Ok(())
}

pub struct UserService {
    pub db: Db,
    pub notifications: Notifications,
}
inventory::submit! {
    bilibi_module_registry::Module {
        id:"core/user",layer:bilibi_module_registry::Layer::Core,
        dependencies:&["base/database","base/security"],migrations:&[MIGRATION],
        services:&[bilibi_module_registry::Service {id:"core/user.accounts",factory:|services|Ok(std::sync::Arc::new(UserService {db:services.get::<Db>("base/database.pool")?.as_ref().clone(),notifications:Notifications::default()}))}],
    }
}

impl UserService {
    pub async fn login(&self, tenant: i64, email: &str, password: &str) -> Result<Login> {
        login(&self.db, tenant, email, password).await
    }
    pub async fn logout(&self, token: &str, csrf: &str) -> Result<()> {
        logout(&self.db, token, csrf).await
    }
    pub async fn mark_read(&self, token: &str) -> Result<()> {
        mark_read(&self.db, token, &self.notifications).await
    }
}
