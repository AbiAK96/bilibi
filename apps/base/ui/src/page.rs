//! Module-owned page adapters. Web establishes access before calling an adapter.
//! Framing is presentation only: it never grants a capability or an identity.
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

pub type PageFuture<'a, Error> = Pin<Box<dyn Future<Output = Result<PageBody, Error>> + Send + 'a>>;

pub struct PageRequest<'a, Context> {
    pub operation: &'static str,
    pub scope: Option<&'a Context>,
    pub path: &'a HashMap<String, String>,
    pub query: &'a HashMap<String, String>,
    pub body: &'a [u8],
    pub framed: bool,
}

/// HTML must be rendered/escaped by the owner. Enable live only with a mount adapter.
pub struct PageBody {
    pub html: String,
    pub live: bool,
}
pub trait Page<Context, Error>: Send + Sync {
    fn render<'a>(&'a self, request: PageRequest<'a, Context>) -> PageFuture<'a, Error>;
    /// Each mount creates independent state, including for two instances of a panel.
    fn mount<'a>(&'a self, _request: PageRequest<'a, Context>) -> MountFuture<'a, Context, Error> {
        Box::pin(async { Ok(None) })
    }
}
/// Registry service type (Web uses Tenancy Scope and Database Error): contribute its service ID in Route.service.
pub struct PageService<Context, Error>(pub Arc<dyn Page<Context, Error>>);

pub type MountFuture<'a, Context, Error> = Pin<
    Box<dyn Future<Output = Result<Option<Box<dyn LiveState<Context, Error>>>, Error>> + Send + 'a>,
>;
pub type LiveFuture<'a, Error> = Pin<Box<dyn Future<Output = Result<LiveBody, Error>> + Send + 'a>>;

/// Only the mounted owner decodes its events. The host never interprets business payloads.
pub enum LiveInput<'a> {
    Refresh,
    Params(&'a HashMap<String, String>),
    Event(&'a serde_json::Value),
}
pub struct LiveRequest<'a, Context> {
    pub scope: &'a Context,
    pub credential: &'a str,
    pub input: LiveInput<'a>,
}
#[derive(Default)]
pub struct LiveBody {
    pub html: String,
    /// Canonical query values; the host supplies the discovered route's path.
    pub query: HashMap<String, String>,
    pub notice: String,
    pub rows: usize,
}
/// Server-owned child mounts. Client events cannot pick an arbitrary embed/service.
/// `id` must name the HTML element containing this panel's complete fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PanelSpec {
    pub id: String,
    pub embed: String,
    pub path: HashMap<String, String>,
    pub query: HashMap<String, String>,
}
/// Broadcast after commit. None means tenant-wide; Some(user) means recipient-only.
#[derive(Clone, Debug)]
pub struct Change {
    pub tenant: i64,
    pub user: Option<i64>,
}
pub struct Subscription {
    pub receiver: tokio::sync::broadcast::Receiver<Change>,
}
/// State and subscriptions are dropped on navigation/disconnect. Every call receives
/// freshly guarded scope. Background refresh never records session activity.
pub trait LiveState<Context, Error>: Send {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Context>) -> LiveFuture<'a, Error>;
    fn panels(&self) -> Vec<PanelSpec> {
        Vec::new()
    }
    fn subscriptions(&mut self) -> Vec<Subscription> {
        Vec::new()
    }
}
