//! Shared PostgreSQL pool, errors, migration runner, and transactional mutation capture.
pub mod seeds;
pub mod timing;
mod transaction;
pub use transaction::{
    Capture, CaptureFuture, Change, Context, MutationBoundary, MutationTransaction, Principal,
};

use deadpool_postgres::{Manager, Pool};
use tokio_postgres::NoTls;

pub use deadpool_postgres::Pool as Db;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Sign in again")]
    Unauthorized,
    #[error("Access denied")]
    Forbidden,
    #[error("Not found")]
    NotFound,
    #[error("Invalid input")]
    Invalid,
    #[error("The record changed. Reload before saving")]
    Conflict,
    #[error("Database unavailable")]
    Database(#[source] Traced<tokio_postgres::Error>),
    #[error("Database unavailable")]
    Pool(#[source] Traced<deadpool_postgres::PoolError>),
    #[error(transparent)]
    Registry(#[from] bilibi_module_registry::RegistryError),
    #[error("Applied production seed definition changed: {0}; publish a new version")]
    SeedDefinition(String),
    #[error("Internal error")]
    Internal,
}

/// A failure from below this crate and, in debug builds only, the stack where
/// it first surfaced: the page that explains an internal error to a developer
/// can then show the query's caller instead of the handler that propagated it.
pub struct Traced<E> {
    error: E,
    origin: Option<std::backtrace::Backtrace>,
}

impl<E> Traced<E> {
    fn new(error: E) -> Self {
        Self {
            error,
            origin: cfg!(debug_assertions).then(std::backtrace::Backtrace::force_capture),
        }
    }
}

/// Debug shows the failure alone; the stack is read through `Error::origin`.
impl<E: std::fmt::Debug> std::fmt::Debug for Traced<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl<E: std::fmt::Display> std::fmt::Display for Traced<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl<E: std::error::Error + 'static> std::error::Error for Traced<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}

impl From<tokio_postgres::Error> for Error {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Database(Traced::new(error))
    }
}

impl From<deadpool_postgres::PoolError> for Error {
    fn from(error: deadpool_postgres::PoolError) -> Self {
        Self::Pool(Traced::new(error))
    }
}

impl Error {
    /// Where a database failure first surfaced; `None` in release builds and
    /// for errors that did not come from the database.
    pub fn origin(&self) -> Option<&std::backtrace::Backtrace> {
        match self {
            Self::Database(traced) => traced.origin.as_ref(),
            Self::Pool(traced) => traced.origin.as_ref(),
            _ => None,
        }
    }
}

pub fn pool(url: &str) -> Result<Db> {
    let config = url.parse().map_err(|_| Error::Invalid)?;
    Pool::builder(Manager::new(config, NoTls))
        .max_size(16)
        .build()
        .map_err(|_| Error::Internal)
}

/// The host supplies module-owned migrations in dependency order. A ledger and
/// advisory transaction lock make concurrent starts safe; DDL is atomic.
pub async fn migrate(db: &Db, migrations: &[(&str, &str)]) -> Result<()> {
    let mut client = db.get().await?;
    let tx = client.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock(841317)", &[])
        .await?;
    tx.batch_execute("CREATE TABLE IF NOT EXISTS bilibi_migrations (version text PRIMARY KEY, applied_at timestamptz NOT NULL DEFAULT now())").await?;
    for (version, sql) in migrations {
        if tx
            .query_opt(
                "SELECT version FROM bilibi_migrations WHERE version=$1",
                &[version],
            )
            .await?
            .is_none()
        {
            tx.batch_execute(sql).await?;
            tx.execute(
                "INSERT INTO bilibi_migrations(version) VALUES ($1)",
                &[version],
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

// Frozen historical bootstrap: future tenancy/session/authz/audit migrations
// belong to their owning module descriptors, never a replay of this version.
pub const MIGRATION: (&str, &str) = ("001-base", include_str!("../migrations/001.sql"));

inventory::submit! {
    bilibi_module_registry::Module {
        id: "base/database", layer: bilibi_module_registry::Layer::Base,
        dependencies: &["base/module_registry"], migrations: &[MIGRATION, ("004-database-boundary", include_str!("../migrations/004-boundary.sql"))],
        services: &[bilibi_module_registry::Service {id:"base/database.mutations", factory: |_| Ok(std::sync::Arc::new(MutationBoundary::default()))}, bilibi_module_registry::Service {id:"base/database.pool", factory: |services| services.get::<Db>("bootstrap.database").map(|value|value as bilibi_module_registry::Value) }],
    }
}
inventory::submit! {
    bilibi_module_registry::Ready { module: "base/database", check: |services| services.get::<MutationBoundary>("base/database.mutations")?.ready() }
}
