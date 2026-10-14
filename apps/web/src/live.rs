//! Module-owned live state. Web only owns routing, guards, targeting and transport.
use crate::{routes, App};
use askama::Template;
use axum::extract::ws::{Message, WebSocket};
use bilibi_database::{Error, Result};
use bilibi_module_registry::contributions::{Access, Embed, EMBEDS};
use bilibi_security::Scope;
use bilibi_ui::page::{
    Change, LiveBody, LiveInput, LiveRequest, LiveState, PageRequest, PanelSpec,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinSet};

type State = Box<dyn LiveState<Scope, Error>>;
type Service = bilibi_ui::page::PageService<Scope, Error>;
fn page_target() -> String {
    "page".into()
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Mount {
        path: String,
        #[serde(default)]
        params: HashMap<String, String>,
    },
    Navigate {
        path: String,
        #[serde(default)]
        params: HashMap<String, String>,
    },
    Params {
        params: HashMap<String, String>,
    },
    #[serde(rename = "event")]
    Action {
        #[serde(default = "page_target")]
        target: String,
        payload: Value,
    },
}
fn decode(text: &str) -> (Option<u64>, Result<Event>) {
    // Separate transport metadata before decoding the strict module intent. Keep
    // valid request IDs even on rejection so the browser can acknowledge errors.
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return (None, Err(Error::Invalid));
    };
    let Some(object) = value.as_object_mut() else {
        return (None, Err(Error::Invalid));
    };
    let request_id = match object.remove("request_id") {
        None | Some(Value::Null) => None,
        Some(value) => match serde_json::from_value::<u64>(value) {
            Ok(id) => Some(id),
            Err(_) => return (None, Err(Error::Invalid)),
        },
    };
    (
        request_id,
        serde_json::from_value(value).map_err(|_| Error::Invalid),
    )
}

struct Wake {
    generation: u64,
    target: String,
    change: Option<Change>,
}
struct Mounted {
    state: State,
    access: Access,
    jobs: JoinSet<()>,
    generation: u64,
}
impl Mounted {
    fn new(
        mut state: State,
        access: Access,
        generation: u64,
        target: &str,
        sender: &mpsc::Sender<Wake>,
    ) -> Self {
        let mut jobs = JoinSet::new();
        for mut subscription in state.subscriptions() {
            let sender = sender.clone();
            let target = target.to_owned();
            jobs.spawn(async move {
                loop {
                    let change = match subscription.receiver.recv().await {
                        Ok(change) => Some(change),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => None,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    if sender
                        .send(Wake {
                            generation,
                            target: target.clone(),
                            change,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        Self {
            state,
            access,
            jobs,
            generation,
        }
    }
}
impl Drop for Mounted {
    fn drop(&mut self) {
        self.jobs.abort_all();
    }
}
struct Panel {
    spec: PanelSpec,
    mounted: Mounted,
}
struct Session {
    framed: bool,
    path: String,
    page: Option<Mounted>,
    panels: HashMap<String, Panel>,
    generation: u64,
    sender: mpsc::Sender<Wake>,
}
impl Session {
    async fn scope(&mut self, app: &App, token: &str) -> Result<Scope> {
        let page = self.page.as_ref().ok_or(Error::Invalid)?;
        routes::check_access(app, page.access, Some(token))
            .await?
            .ok_or(Error::Forbidden)
    }
    async fn mount(
        &mut self,
        app: &App,
        token: &str,
        path: String,
        query: HashMap<String, String>,
    ) -> Result<Vec<Value>> {
        let (bound, params) = routes::resolve(app, &path)?;
        if self.framed && !bound.route.framed {
            return Err(Error::Forbidden);
        }
        let scope = routes::check_access(app, bound.route.access, Some(token))
            .await?
            .ok_or(Error::Forbidden)?;
        let state = bound
            .page
            .0
            .mount(PageRequest {
                operation: bound.route.operation,
                scope: Some(&scope),
                path: &params,
                query: &query,
                body: &[],
                framed: self.framed,
            })
            .await?
            .ok_or(Error::Invalid)?;
        // Old state/subscribers cannot survive a route change or receive queued wakes.
        self.panels.clear();
        self.generation += 1;
        self.page = Some(Mounted::new(
            state,
            bound.route.access,
            self.generation,
            "page",
            &self.sender,
        ));
        self.path = path;
        self.update(app, token, "page", LiveInput::Refresh).await
    }
    async fn update(
        &mut self,
        app: &App,
        token: &str,
        target: &str,
        input: LiveInput<'_>,
    ) -> Result<Vec<Value>> {
        let scope = self.scope(app, token).await?;
        if target == "shell" {
            if self.framed {
                return Err(Error::Invalid);
            }
            let LiveInput::Event(payload) = input else {
                return Err(Error::Invalid);
            };
            if payload != &json!({"type":"read"}) {
                return Err(Error::Invalid);
            }
            app.user.mark_read(token).await?;
            let body = self
                .page
                .as_mut()
                .ok_or(Error::Invalid)?
                .state
                .update(LiveRequest {
                    scope: &scope,
                    credential: token,
                    input: LiveInput::Refresh,
                })
                .await?;
            let mut message = self.page_message(app, &scope, body).await?;
            message["notice"] = json!("Notifications marked read");
            let mut messages = vec![message];
            messages.extend(self.sync_panels(app, token, &scope).await?);
            return Ok(messages);
        }
        if target == "page" {
            let body = self
                .page
                .as_mut()
                .ok_or(Error::Invalid)?
                .state
                .update(LiveRequest {
                    scope: &scope,
                    credential: token,
                    input,
                })
                .await?;
            let mut messages = vec![self.page_message(app, &scope, body).await?];
            messages.extend(self.sync_panels(app, token, &scope).await?);
            return Ok(messages);
        }
        let panel = self.panels.get_mut(target).ok_or(Error::Invalid)?;
        routes::check_access(app, panel.mounted.access, Some(token)).await?;
        let body = panel
            .mounted
            .state
            .update(LiveRequest {
                scope: &scope,
                credential: token,
                input,
            })
            .await?;
        Ok(vec![fragment(target, body)])
    }
    async fn page_message(&mut self, app: &App, scope: &Scope, body: LiveBody) -> Result<Value> {
        let html = if self.framed {
            bilibi_ui::Framed {
                body: &body.html,
                live: true,
            }
            .render()
            .map_err(|_| Error::Internal)?
        } else {
            let client = app.db.get().await?;
            let unread = match bilibi_user::unread(&**client, scope).await {
                Ok(value) => value,
                Err(Error::Forbidden) => 0,
                Err(error) => return Err(error),
            };
            bilibi_ui::Shell {
                user: &scope.user_name,
                email: &scope.email,
                tenant: &scope.tenant_name,
                csrf: &scope.csrf,
                unread,
                live: true,
                body: &body.html,
            }
            .render()
            .map_err(|_| Error::Internal)?
        };
        let mut pairs: Vec<_> = body.query.iter().collect();
        pairs.sort_by_key(|(key, _)| *key);
        let query = pairs
            .into_iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let url = if query.is_empty() {
            self.path.clone()
        } else {
            format!("{}?{query}", self.path)
        };
        Ok(
            json!({"type":"page", "target":"app", "html":html, "url":url, "notice":body.notice, "rows":body.rows}),
        )
    }
    async fn sync_panels(&mut self, app: &App, token: &str, scope: &Scope) -> Result<Vec<Value>> {
        let specs = self.page.as_ref().ok_or(Error::Invalid)?.state.panels();
        if specs.len() > 32 {
            return Err(Error::Invalid);
        }
        let mut ids = std::collections::HashSet::new();
        for spec in &specs {
            if spec.id.is_empty()
                || spec.id.len() > 128
                || ["page", "shell", "app"].contains(&spec.id.as_str())
                || !spec
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                || !ids.insert(&spec.id)
            {
                return Err(Error::Invalid);
            }
        }
        self.panels
            .retain(|id, panel| specs.iter().any(|s| &s.id == id && s == &panel.spec));
        let mut messages = Vec::new();
        for spec in specs {
            let id = spec.id.clone();
            let result = async {
                if !self.panels.contains_key(&id) {
                    let embed = app
                        .services
                        .catalog()?
                        .entries::<Embed>(EMBEDS)?
                        .iter()
                        .find(|e| e.id == spec.embed)
                        .ok_or(Error::NotFound)?
                        .get::<Embed>()?;
                    routes::check_access(app, embed.access, Some(token)).await?;
                    let service: Arc<Service> = app.services.get(embed.service)?;
                    let state = service
                        .0
                        .mount(PageRequest {
                            operation: embed.operation,
                            scope: Some(scope),
                            path: &spec.path,
                            query: &spec.query,
                            body: &[],
                            framed: self.framed,
                        })
                        .await?
                        .ok_or(Error::Invalid)?;
                    self.generation += 1;
                    let mounted =
                        Mounted::new(state, embed.access, self.generation, &id, &self.sender);
                    self.panels.insert(id.clone(), Panel { spec, mounted });
                }
                let panel = self.panels.get_mut(&id).ok_or(Error::Internal)?;
                routes::check_access(app, panel.mounted.access, Some(token)).await?;
                panel
                    .mounted
                    .state
                    .update(LiveRequest {
                        scope,
                        credential: token,
                        input: LiveInput::Refresh,
                    })
                    .await
            }
            .await;
            match result {
                Ok(body) => messages.push(fragment(&id, body)),
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    self.panels.remove(&id);
                    messages.push(json!({"type":"error", "target":id, "message":error.to_string(), "clear":true}));
                }
            }
        }
        Ok(messages)
    }
    async fn event(&mut self, app: &App, token: &str, event: Event) -> Result<Vec<Value>> {
        let messages = match event {
            Event::Mount { path, params } | Event::Navigate { path, params } => {
                self.mount(app, token, path, params).await?
            }
            Event::Params { params } => {
                self.update(app, token, "page", LiveInput::Params(&params))
                    .await?
            }
            Event::Action { target, payload } => {
                self.update(app, token, &target, LiveInput::Event(&payload))
                    .await?
            }
        };
        let client = app.db.get().await?;
        app.security.sessions.activity(&**client, token).await?;
        Ok(messages)
    }
}
fn fragment(id: &str, body: LiveBody) -> Value {
    json!({"type":"fragment", "target":id, "html":body.html, "notice":body.notice, "rows":body.rows})
}
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn fatal(error: &Error) -> bool {
    matches!(
        error,
        Error::Unauthorized
            | Error::Forbidden
            | Error::NotFound
            | Error::Database(_)
            | Error::Pool(_)
            | Error::Internal
    )
}

pub async fn run(app: App, token: String, mut socket: WebSocket, framed: bool) {
    let (sender, mut wakes) = mpsc::channel(64);
    let mut session = Session {
        framed,
        path: String::new(),
        page: None,
        panels: HashMap::new(),
        generation: 0,
        sender,
    };
    let mut notifications = app.notifications.subscribe();
    let mut refresh = tokio::time::interval(Duration::from_secs(15));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    refresh.tick().await;
    loop {
        let mut request_id = None;
        let mut target = "page".to_owned();
        let mut history = "replace";
        let result = tokio::select! {
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => {
                    let (id, event) = decode(&text);
                    request_id = id;
                    match event {
                        Ok(event) => {
                            if let Event::Action {target: id, ..} = &event {target = id.clone();}
                            if matches!(event, Event::Navigate {..} | Event::Params {..} | Event::Action {..}) {history = "push";}
                            session.event(&app, &token, event).await
                        },
                        Err(error) => Err(error),
                    }
                },
                Some(Ok(Message::Ping(data))) => { if socket.send(Message::Pong(data)).await.is_err() {break;} continue; },
                Some(Ok(Message::Pong(_))) => continue,
                _ => break,
            },
            wake = wakes.recv() => {
                let Some(wake) = wake else {break;};
                let mounted = if wake.target == "page" { session.page.as_ref() } else { session.panels.get(&wake.target).map(|p| &p.mounted) };
                if !mounted.is_some_and(|m| m.generation == wake.generation) {continue;}
                target = wake.target;
                match session.scope(&app, &token).await {
                    Ok(scope) => {
                        if wake.change.as_ref().is_some_and(|c| c.tenant != scope.tenant() || c.user.is_some_and(|user| user != scope.user())) {continue;}
                        session.update(&app, &token, &target, LiveInput::Refresh).await
                    },
                    Err(error) => Err(error),
                }
            },
            notification = notifications.recv() => {
                if session.page.is_none() {continue;}
                match session.scope(&app, &token).await {
                    Ok(scope) => {
                        match notification {
                            Ok(event) if event.tenant != scope.tenant() || event.user != scope.user() => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            _ => {},
                        }
                        session.update(&app, &token, "page", LiveInput::Refresh).await
                    },
                    Err(error) => Err(error),
                }
            },
            _ = refresh.tick() => {
                if session.page.is_none() {continue;}
                session.update(&app, &token, "page", LiveInput::Refresh).await
            },
        };
        let (mut messages, close) = match result {
            Ok(messages) => (messages, false),
            Err(error) => {
                let panel = target != "page" && target != "shell";
                let clear = fatal(&error);
                let parent_denied =
                    session.page.is_some() && session.scope(&app, &token).await.is_err();
                let close =
                    clear && (!panel || parent_denied || matches!(error, Error::Unauthorized));
                if panel && clear {
                    session.panels.remove(&target);
                }
                (
                    vec![
                        json!({"type":"error", "target":if close {"app"} else {&target}, "message":error.to_string(), "clear":clear}),
                    ],
                    close,
                )
            }
        };
        for message in &mut messages {
            message["request_id"] = json!(request_id);
            if message["type"] == "page" {
                message["history"] = json!(history);
            }
            if socket
                .send(Message::Text(message.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
        }
        if close {
            break;
        }
    }
    // Dropping Session aborts all module subscription tasks and drops state/receivers.
}
