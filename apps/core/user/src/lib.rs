//! Credentials, server sessions, preferences, and the user's notifications and bell.
mod appearance;
mod contributions;
pub mod database_queries;
mod directory;
mod inbox;
pub mod notifications;
pub mod password;
mod preferences;
mod query_ui;
mod reset;
mod verification;
pub mod web;

use bilibi_database::{Db, Error, MutationBoundary, Result};
use bilibi_security::{authorize, guard, Capability, Sessions};
pub use notifications::{unread, Notifications};
pub use password::PolicyViolation;
pub use preferences::{
    list_pins, normalize_label, normalize_url, presentation, Pin, PreferenceError, Presentation,
    MAX_PINS, THEMES, THEME_KEY,
};
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
pub const NOTIFICATIONS_MIGRATION: (&str, &str) = (
    "core-user-005-notifications",
    include_str!("../migrations/005-notifications.sql"),
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

pub async fn logout(db: &Db, token: &str, csrf: &str, sessions: &Sessions) -> Result<()> {
    let client = db.get().await?;
    let scope = guard(&**client, token, sessions).await?;
    if scope.csrf != csrf {
        return Err(Error::Forbidden);
    }
    authorize(&**client, &scope, Capability::SessionLogout).await?;
    // Through Session, so every other live page of this session is signed out.
    sessions.delete(&**client, &scope, token).await
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
        dependencies:&["base/database","base/security","base/notifications","base/tenancy","base/session","base/principal_directory","base/mail","base/settings","base/datetime","base/locale","base/ui","base/audit","base/authz","base/menu"],migrations:&[MIGRATION,CREDENTIALS_MIGRATION,PINS_MIGRATION,NOTIFICATIONS_MIGRATION,database_queries::MIGRATION],
        services:&[bilibi_module_registry::Service {id:"core/user.database_queries",factory:|services| {
            Ok(std::sync::Arc::new(database_queries::QueryService::new(services.get::<Db>("base/database.pool")?.as_ref().clone(), services.get("base/database.mutations")?, services.get("base/tenancy.authentication")?)))
        }},bilibi_module_registry::Service {id:"core/user.query-pages",factory:|services| {
            Ok(std::sync::Arc::new(bilibi_ui::page::PageService(std::sync::Arc::new(query_ui::Pages {
                queries:services.get("core/user.database_queries")?, db:services.get::<Db>("base/database.pool")?.as_ref().clone(),
                display:services.get("base/datetime.display")?,locales:services.get("base/locale.locales")?,
            }))))
        }},bilibi_module_registry::Service {id:"core/user.accounts",factory:|services| {
            let notifications = Notifications::default();
            services.get::<bilibi_security::Authentication>("base/tenancy.authentication")?.bind(std::sync::Arc::new(AccountVerifier))?;
            services.get::<bilibi_notifications::NotificationBoundary>("base/notifications.sink")?.bind(std::sync::Arc::new(notifications.clone()))?;
            services.get::<bilibi_principal_directory::PrincipalDirectory>("base/principal_directory.directory")?.bind("core/user", std::sync::Arc::new(directory::UserDirectory))?;
            Ok(std::sync::Arc::new(UserService {sessions:services.get("base/session.sessions")?,mutations:services.get("base/database.mutations")?,db:services.get::<Db>("base/database.pool")?.as_ref().clone(),notifications,mailer:services.get("base/mail.mailer")?,settings:services.get("base/settings.store")?,display:services.get("base/datetime.display")?,locales:services.get("base/locale.locales")?,signer:Signer::from_hex(std::env::var("BILIBI_USER_SIGNING_KEY").ok().as_deref()),origin:std::env::var("APP_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into()).trim_end_matches('/').to_owned()}))
        }},
        bilibi_module_registry::Service {id:"core/user.pages",factory:|services| {
            Ok(std::sync::Arc::new(bilibi_ui::page::PageService(std::sync::Arc::new(web::Pages {users:services.get("core/user.accounts")?,display:services.get("base/datetime.display")?,locales:services.get("base/locale.locales")?}))))
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
    pub async fn mark_read(&self, token: &str, id: i64) -> Result<notifications::Item> {
        notifications::mark_read(
            &self.db,
            token,
            id,
            &self.notifications,
            &self.mutations,
            &self.sessions,
        )
        .await
    }
    pub async fn mark_all_read(&self, token: &str) -> Result<u64> {
        notifications::mark_all_read(
            &self.db,
            token,
            &self.notifications,
            &self.mutations,
            &self.sessions,
        )
        .await
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
    fn confirm_password<'a>(
        &'a self,
        client: &'a dyn bilibi_security::ProofClient,
        tenant: i64,
        user: i64,
        password: &'a str,
    ) -> bilibi_security::ProofFuture<'a, bool> {
        Box::pin(async move {
            if password.len() > password::MAX_LENGTH {
                return Ok(false);
            }
            let stored: Option<String> = client.query_opt("SELECT password_hash FROM users WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL AND status='active'", &[&tenant,&user]).await?.map(|row| row.get(0));
            password::verify(password, stored.as_deref()).await
        })
    }
}
