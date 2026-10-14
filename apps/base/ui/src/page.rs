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

/// HTML must be rendered/escaped by the owner. Only pages with a socket adapter
/// should enable live updates; F5 supplies the generic state/event contract.
pub struct PageBody {
    pub html: String,
    pub live: bool,
}
pub trait Page<Context, Error>: Send + Sync {
    fn render<'a>(&'a self, request: PageRequest<'a, Context>) -> PageFuture<'a, Error>;
}
/// Registry service type (Web uses Tenancy Scope and Database Error): contribute its service ID in Route.service.
pub struct PageService<Context, Error>(pub Arc<dyn Page<Context, Error>>);
