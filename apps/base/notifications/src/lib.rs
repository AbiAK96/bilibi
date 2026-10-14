//! Notification boundary only: Core User owns persistence and delivery. A
//! business owner records in its own transaction, then publishes after commit.
use bilibi_database::Result;
use bilibi_security::Scope;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};
use tokio_postgres::Transaction;

pub type RecordFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
pub trait NotificationSink: Send + Sync {
    fn record<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        message: &'a str,
    ) -> RecordFuture<'a>;
    fn committed(&self, scope: &Scope);
}

/// Bind exactly once during service installation. An unbound sink fails boot;
/// callers never silently lose a notification or commit without its audit.
#[derive(Default)]
pub struct NotificationBoundary {
    sink: OnceLock<Arc<dyn NotificationSink>>,
}
impl NotificationBoundary {
    pub fn bind(&self, sink: Arc<dyn NotificationSink>) -> bilibi_module_registry::Result<()> {
        self.sink.set(sink).map_err(|_| {
            bilibi_module_registry::RegistryError::Service(
                "duplicate notification sink binding".into(),
            )
        })
    }
    fn sink(&self) -> bilibi_module_registry::Result<&dyn NotificationSink> {
        self.sink.get().map(|sink| sink.as_ref()).ok_or_else(|| {
            bilibi_module_registry::RegistryError::Service("unbound notification sink".into())
        })
    }
    pub async fn record(&self, tx: &Transaction<'_>, scope: &Scope, message: &str) -> Result<()> {
        self.sink()?.record(tx, scope, message).await
    }
    pub fn committed(&self, scope: &Scope) -> Result<()> {
        self.sink()?.committed(scope);
        Ok(())
    }
}
inventory::submit! {
    bilibi_module_registry::Module {
        id: "base/notifications", layer: bilibi_module_registry::Layer::Base,
        dependencies: &["base/module_registry", "base/database", "base/security"],
        migrations: &[],
        services: &[bilibi_module_registry::Service { id: "base/notifications.sink", factory: |_| Ok(Arc::new(NotificationBoundary::default())) }],
    }
}
inventory::submit! {
    bilibi_module_registry::Ready {
        module: "base/notifications",
        check: |services| services.get::<NotificationBoundary>("base/notifications.sink")?.sink().map(|_| ()),
    }
}
