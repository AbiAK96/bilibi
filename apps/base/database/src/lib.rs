//! Shared PostgreSQL pool, errors, migration runner, and transactional mutation capture.
pub mod seeds;
mod transaction;
pub use transaction::{
    Capture, CaptureFuture, Change, Context, DemoCapture, MutationTransaction, Principal,
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
    Database(#[from] tokio_postgres::Error),
    #[error("Database unavailable")]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error(transparent)]
    Registry(#[from] bilibi_module_registry::RegistryError),
    #[error("Applied production seed definition changed: {0}; publish a new version")]
    SeedDefinition(String),
    #[error("Internal error")]
    Internal,
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
        services: &[bilibi_module_registry::Service {id:"base/database.pool", factory: |services| services.get::<Db>("bootstrap.database").map(|value|value as bilibi_module_registry::Value) }],
    }
}
