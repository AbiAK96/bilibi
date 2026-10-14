//! Manual live-protocol specimen, deliberately separate from product routes.
//! Run with an isolated, migrated/demo-seeded DATABASE_URL and APP_ORIGIN.
use bilibi_database::{Error, Result};
use bilibi_module_registry::{
    contributions::{Access, Contribution, Embed, EmbedContext, Method, Route, EMBEDS, ROUTES},
    Layer, Module, Provenance, Service,
};
use bilibi_security::Scope;
use bilibi_ui::page::{
    Change, LiveBody, LiveFuture, LiveInput, LiveRequest, LiveState, MountFuture, Page, PageBody,
    PageFuture, PageRequest, PageService, PanelSpec, Subscription,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::broadcast;

struct Reference {
    changes: broadcast::Sender<Change>,
}
struct ReferenceState {
    id: Option<&'static str>,
    count: u64,
    changes: broadcast::Sender<Change>,
}
fn body(id: Option<&str>, count: u64) -> String {
    match id {
        None => "<section id=live-reference><h1>Live page and panels</h1><p>Each panel owns its count. An announcement refreshes both through a recipient-scoped subscription.</p><button type=button data-event=announce>Announce to panels</button><div id=counter-a></div><div id=counter-b></div></section>".into(),
        Some(id) => format!("<section id={id}><h2>{id}</h2><p data-count>{count}</p><button type=button data-event=increment data-target={id}>Increment</button></section>"),
    }
}
impl Page<Scope, Error> for Reference {
    fn render<'a>(&'a self, _request: PageRequest<'a, Scope>) -> PageFuture<'a, Error> {
        Box::pin(async {
            Ok(PageBody {
                html: body(None, 0),
                live: true,
            })
        })
    }
    fn mount<'a>(&'a self, request: PageRequest<'a, Scope>) -> MountFuture<'a, Scope, Error> {
        Box::pin(async move {
            let id = match request.operation {
                "page" => None,
                "counter" => match request.path.get("id").map(String::as_str) {
                    Some("counter-a") => Some("counter-a"),
                    Some("counter-b") => Some("counter-b"),
                    _ => return Err(Error::Invalid),
                },
                _ => return Err(Error::NotFound),
            };
            let state: Box<dyn LiveState<Scope, Error>> = Box::new(ReferenceState {
                id,
                count: 0,
                changes: self.changes.clone(),
            });
            Ok(Some(state))
        })
    }
}
impl LiveState<Scope, Error> for ReferenceState {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            let mut notice = String::new();
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(values) if values.is_empty() => {}
                LiveInput::Event(value)
                    if value == &serde_json::json!({"type":"increment"}) && self.id.is_some() =>
                {
                    self.count += 1;
                }
                LiveInput::Event(value)
                    if value == &serde_json::json!({"type":"announce"}) && self.id.is_none() =>
                {
                    let _ = self.changes.send(Change {
                        tenant: request.scope.tenant(),
                        user: Some(request.scope.user()),
                    });
                    notice = "Recipient-scoped announcement".into();
                }
                _ => return Err(Error::Invalid),
            }
            Ok(LiveBody {
                html: body(self.id, self.count),
                notice,
                ..LiveBody::default()
            })
        })
    }
    fn panels(&self) -> Vec<PanelSpec> {
        if self.id.is_some() {
            return Vec::new();
        }
        ["counter-a", "counter-b"]
            .into_iter()
            .map(|id| PanelSpec {
                id: id.into(),
                embed: "extensions/live_reference.counter".into(),
                path: HashMap::from([("id".into(), id.into())]),
                query: HashMap::new(),
            })
            .collect()
    }
    fn subscriptions(&mut self) -> Vec<Subscription> {
        if self.id.is_none() {
            return Vec::new();
        }
        vec![Subscription {
            receiver: self.changes.subscribe(),
        }]
    }
}
inventory::submit! {
    Module {id: "extensions/live_reference", layer: Layer::Extensions,
        dependencies: &["base/ui"], migrations: &[], services: &[Service {
            id: "extensions/live_reference.pages", factory: |_| {
                let (changes, _) = broadcast::channel(16);
                Ok(Arc::new(PageService::<Scope, Error>(Arc::new(Reference {changes}))))
            },
        }],
    }
}
inventory::submit! {
    Contribution::new(ROUTES, "extensions/live_reference.page", Provenance {module:"extensions/live_reference", source:file!(), line:line!()}, &Route {
        method:Method::Get, path:"/live-reference", service:"extensions/live_reference.pages", operation:"page", access:Access::SignedIn, framed:true,
    })
}
inventory::submit! {
    Contribution::new(EMBEDS, "extensions/live_reference.counter", Provenance {module:"extensions/live_reference", source:file!(), line:line!()}, &Embed {
        slot:"reference.counter", context:EmbedContext::Record {kind:"reference"}, service:"extensions/live_reference.pages", operation:"counter", access:Access::Capability("notifications.view"),
    })
}
#[tokio::main]
async fn main() -> Result<()> {
    let db = bilibi_database::pool(&std::env::var("DATABASE_URL").map_err(|_| Error::Invalid)?)?;
    let origin = std::env::var("APP_ORIGIN").map_err(|_| Error::Invalid)?;
    let bind = std::env::var("BIND_ADDR").map_err(|_| Error::Invalid)?;
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| Error::Internal)?;
    println!("Live reference listening at {origin}");
    axum::serve(
        listener,
        bilibi_web::router(bilibi_web::App::new(db, &origin)?),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .map_err(|_| Error::Internal)
}
