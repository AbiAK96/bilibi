//! Credentials, server sessions, and the user's notification bell.
mod appearance;
mod contributions;
mod directory;
pub mod password;
mod preferences;
mod reset;
mod verification;
pub mod web;

use bilibi_database::{Change, Db, Error, MutationBoundary, MutationTransaction, Result};
use bilibi_security::{authorize, guard, Capability, Scope, Sessions};
pub use password::PolicyViolation;
pub use preferences::{
    list_pins, normalize_label, normalize_url, presentation, Pin, PreferenceError, Presentation,
    MAX_PINS, THEMES, THEME_KEY,
};
use serde_json::json;
use tokio::sync::broadcast;
use tokio_postgres::GenericClient;
pub use verification::{Signer, Verification};

pub const MIGRATION: (&str, &str) = ("002-user", include_str!("../migrations/002.sql"));
pub const CREDENTIALS_MIGRATION: (&str, &str) = (
    "core-user-003-credentials",
    include_str!("../migrations/003-credentials.sql"),
);
pub const PINS_MIGRATION: (&str, &str) = (
    "core-user-004-pins",
    include_str!("../migrations/004-pins.sql"),
);

/// Blocking Argon2id hash for seeds and tools; services use `password::hash`.
pub fn hash_password(password: &str) -> Result<String> {
    password::hash_blocking(password)
}

/// What a credential flow can refuse with, beyond infrastructure failures.
/// Pages map each variant to a field or flash message; none reveals whether
/// an account exists.
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("is incorrect")]
    IncorrectPassword,
    #[error("{0}")]
    Policy(PolicyViolation),
    #[error("This link is invalid or has expired")]
    InvalidOrExpiredToken,
    #[error("Email is already verified")]
    AlreadyVerified,
    #[error("Email verification is not configured")]
    VerificationUnavailable,
    #[error("The email could not be sent")]
    Delivery,
    #[error(transparent)]
    Other(#[from] Error),
}
impl From<tokio_postgres::Error> for CredentialError {
    fn from(error: tokio_postgres::Error) -> Self {
        Error::from(error).into()
    }
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
    // An unknown account still pays for one verification.
    let stored: Option<String> = row.as_ref().map(|r| r.get(1));
    if !password::verify(password, stored.as_deref()).await? {
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
pub async fn saved_notification(
    tx: &mut MutationTransaction<'_>,
    scope: &Scope,
    message: &str,
) -> Result<()> {
    if tx.context().tenant != scope.tenant()
        || tx.context().actor.kind != "user"
        || tx.context().actor.id != scope.user().to_string()
    {
        return Err(Error::Forbidden);
    }
    let tenant = scope.tenant();
    let user = scope.user();
    let message = message.to_owned();
    tx.mutate(move |sql| {
        Box::pin(async move {
            let row = sql.query_one(
            "INSERT INTO notifications(tenant_id,user_id,message) VALUES($1,$2,$3) RETURNING id",
            &[&tenant, &user, &message],
        ).await?;
            Ok((
                (),
                vec![Change {
                    kind: "notification".into(),
                    id: row.get(0),
                    old: json!({}),
                    new: json!({"message":message}),
                }],
            ))
        })
    })
    .await
}

pub async fn mark_read(
    db: &Db,
    token: &str,
    notifications: &Notifications,
    mutations: &MutationBoundary,
    sessions: &Sessions,
) -> Result<()> {
    let mut client = db.get().await?;
    let scope = guard(&**client, token, sessions).await?;
    let mut tx = mutations
        .begin(
            &mut client,
            scope.mutation_context("notifications.mark_read"),
        )
        .await?;
    let scope = guard(tx.sql(), token, sessions).await?;
    authorize(tx.sql(), &scope, Capability::NotificationsUpdate).await?;
    let rows = tx.sql().query("SELECT id FROM notifications WHERE tenant_id=$1 AND user_id=$2 AND read_at IS NULL FOR UPDATE", &[&scope.tenant(),&scope.user()]).await?;
    for row in rows {
        let id: i64 = row.get(0);
        let tenant = scope.tenant();
        let user = scope.user();
        tx.mutate(move |sql| Box::pin(async move {
            sql.execute("UPDATE notifications SET read_at=now() WHERE tenant_id=$1 AND user_id=$2 AND id=$3", &[&tenant,&user,&id]).await?;
            Ok(((), vec![Change { kind: "notification".into(), id, old: json!({"read":false}), new: json!({"read":true}) }]))
        })).await?;
    }
    let notifications = notifications.clone();
    tx.after_commit(move || notifications.changed(&scope));
    tx.commit().await
}

pub async fn logout(db: &Db, token: &str, csrf: &str, sessions: &Sessions) -> Result<()> {
    let client = db.get().await?;
    let scope = guard(&**client, token, sessions).await?;
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
    pub sessions: std::sync::Arc<Sessions>,
    pub db: Db,
    pub notifications: Notifications,
    pub mutations: std::sync::Arc<MutationBoundary>,
    mailer: std::sync::Arc<bilibi_mail::Mailer>,
    settings: std::sync::Arc<bilibi_settings::Settings>,
    display: std::sync::Arc<bilibi_datetime::DisplayPolicy>,
    locales: std::sync::Arc<bilibi_locale::Locales>,
    signer: Signer,
    /// Public origin that emailed links point at (`APP_ORIGIN`).
    origin: String,
}
inventory::submit! {
    bilibi_module_registry::Module {
        id:"core/user",layer:bilibi_module_registry::Layer::Core,
        dependencies:&["base/database","base/security","base/notifications","base/tenancy","base/session","base/principal_directory","base/mail","base/settings","base/datetime","base/locale"],migrations:&[MIGRATION,CREDENTIALS_MIGRATION,PINS_MIGRATION],
        services:&[bilibi_module_registry::Service {id:"core/user.accounts",factory:|services| {
            let notifications = Notifications::default();
            services.get::<bilibi_security::Authentication>("base/tenancy.authentication")?.bind(std::sync::Arc::new(AccountVerifier))?;
            services.get::<bilibi_notifications::NotificationBoundary>("base/notifications.sink")?.bind(std::sync::Arc::new(notifications.clone()))?;
            services.get::<bilibi_principal_directory::PrincipalDirectory>("base/principal_directory.directory")?.bind("core/user", std::sync::Arc::new(directory::UserDirectory))?;
            Ok(std::sync::Arc::new(UserService {sessions:services.get("base/session.sessions")?,mutations:services.get("base/database.mutations")?,db:services.get::<Db>("base/database.pool")?.as_ref().clone(),notifications,mailer:services.get("base/mail.mailer")?,settings:services.get("base/settings.store")?,display:services.get("base/datetime.display")?,locales:services.get("base/locale.locales")?,signer:Signer::from_hex(std::env::var("BILIBI_USER_SIGNING_KEY").ok().as_deref()),origin:std::env::var("APP_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into()).trim_end_matches('/').to_owned()}))
        }},
        bilibi_module_registry::Service {id:"core/user.pages",factory:|services| {
            Ok(std::sync::Arc::new(bilibi_ui::page::PageService(std::sync::Arc::new(web::Pages {users:services.get("core/user.accounts")?}))))
        }}],
    }
}

impl UserService {
    pub async fn login(&self, tenant: i64, email: &str, password: &str) -> Result<Login> {
        login(&self.db, tenant, email, password).await
    }
    pub async fn logout(&self, token: &str, csrf: &str) -> Result<()> {
        logout(&self.db, token, csrf, &self.sessions).await
    }
    pub async fn mark_read(&self, token: &str) -> Result<()> {
        mark_read(
            &self.db,
            token,
            &self.notifications,
            &self.mutations,
            &self.sessions,
        )
        .await
    }
}

impl bilibi_notifications::NotificationSink for Notifications {
    fn record<'a>(
        &'a self,
        tx: &'a mut MutationTransaction<'_>,
        scope: &'a Scope,
        message: &'a str,
    ) -> bilibi_notifications::RecordFuture<'a> {
        Box::pin(async move { saved_notification(tx, scope, message).await })
    }
    fn committed(&self, scope: &Scope) {
        self.changed(scope);
    }
}

/// The current account adapter deliberately supplies no invented party links.
/// K9 binds Person/Organisation/Affiliation through their owner APIs.
pub struct AccountVerifier;
impl bilibi_security::ActorVerifier for AccountVerifier {
    fn verify<'a>(
        &'a self,
        client: &'a dyn bilibi_security::ProofClient,
        tenant: i64,
        user: i64,
    ) -> bilibi_security::ProofFuture<'a, bilibi_security::AccountIdentity> {
        Box::pin(async move {
            let row = client.query_opt("SELECT name,email FROM users WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL AND status='active'", &[&tenant,&user]).await?.ok_or(Error::Unauthorized)?;
            Ok(bilibi_security::AccountIdentity {
                name: row.get(0),
                email: row.get(1),
                person: None,
                organisation: None,
                affiliation: None,
            })
        })
    }
}
