//! Module-owned live state. Web only owns routing, guards, targeting and transport.
use crate::{routes, App};
use axum::extract::ws::{Message, WebSocket};
use bilibi_base::perf::{Finish, Kind, Outcome};
use bilibi_database::{Error, Result};
use bilibi_module_registry::contributions::{Access, Embed, EMBEDS};
use bilibi_security::Scope;
use bilibi_ui::page::{
    Change, LiveBody, LiveInput, LiveRequest, LiveState, PageRequest, PanelSpec,
};
use bilibi_ui::params;
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
    /// `path` is the browser's pathname after Back or Forward inside a page
    /// marked `data-history="params"`; the host keeps the mounted state when
    /// that path belongs to the same page and remounts otherwise.
    Params {
        params: HashMap<String, String>,
        #[serde(default)]
        path: Option<String>,
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

/// Who a live socket acts for: the session's account, its operator while
/// impersonating, and the CSRF secret the page was rendered with. The secret
/// rotates on every impersonation switch, so a socket left over from before
/// one (another tab of the same session) is stale even when it switched back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identity {
    user: i64,
    impersonator: Option<i64>,
    csrf: String,
}
impl Identity {
    pub fn of(scope: &Scope) -> Self {
        Self {
            user: scope.user(),
            impersonator: scope.impersonator().map(|operator| operator.user()),
            csrf: scope.csrf.clone(),
        }
    }
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
    identity: Identity,
    /// Set when the session no longer holds `identity`: the socket then
    /// reloads its page under the session's current identity instead of
    /// acting, and closes.
    stale: bool,
    framed: bool,
    path: String,
    /// Route template of the mounted page, the stable identity Perf records.
    route: String,
    /// The discovered route the page mounted under, which decides whether a
    /// path the page or the browser names is still this page.
    page_route: Option<&'static bilibi_module_registry::contributions::Route>,
    page: Option<Mounted>,
    panels: HashMap<String, Panel>,
    /// `EmbedContext::Shell` panels (the bell), keyed by their element id.
    /// They outlive navigation and are never mounted in a framed page.
    shell: HashMap<String, Panel>,
    generation: u64,
    sender: mpsc::Sender<Wake>,
    /// The address the page last answered with: path and canonical query.
    canonical: String,
    /// The browser's own query on a mount or a Back step, which the next
    /// page reply keeps in place of the canonical one when that only adds
    /// values the address left out (Bilimbi patches the URL only on change).
    address: Option<HashMap<String, String>>,
}
impl Session {
    async fn scope(&mut self, app: &App, token: &str) -> Result<Scope> {
        let page = self.page.as_ref().ok_or(Error::Invalid)?;
        let scope = routes::check_access(app, page.access, Some(token))
            .await?
            .ok_or(Error::Forbidden)?;
        self.bound(scope)
    }
    /// Refuses a scope the socket was not opened for; nothing acts on it.
    fn bound(&mut self, scope: Scope) -> Result<Scope> {
        if Identity::of(&scope) != self.identity {
            self.stale = true;
            return Err(Error::Unauthorized);
        }
        Ok(scope)
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
        let scope = self.bound(scope)?;
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
        self.route = bound.route.path.to_owned();
        self.page_route = Some(bound.route);
        self.address = Some(query);
        self.sync_shell(app, token, &scope).await?;
        self.update(app, token, "page", LiveInput::Refresh).await
    }
    /// Moves the mounted page to `path` without remounting, when that path is
    /// served by the same service and operation (a saved layout's address under
    /// the workspace), guarding the new route's access first. Returns the
    /// route's path parameters, or `None` when the path is another page's.
    async fn relocate(
        &mut self,
        app: &App,
        token: &str,
        path: &str,
    ) -> Result<Option<HashMap<String, String>>> {
        let (bound, params) = routes::resolve(app, path)?;
        let current = self.page_route.ok_or(Error::Invalid)?;
        if bound.route.service != current.service
            || bound.route.operation != current.operation
            || (self.framed && !bound.route.framed)
        {
            return Ok(None);
        }
        let scope = routes::check_access(app, bound.route.access, Some(token))
            .await?
            .ok_or(Error::Forbidden)?;
        self.bound(scope)?;
        self.path = path.to_owned();
        self.route = bound.route.path.to_owned();
        self.page_route = Some(bound.route);
        if let Some(page) = &mut self.page {
            page.access = bound.route.access;
        }
        Ok(Some(params))
    }
    /// Mounts each shell embed the actor may see, once per socket, and drops
    /// one whose access is gone. A refused embed renders nothing in the shell.
    async fn sync_shell(&mut self, app: &App, token: &str, scope: &Scope) -> Result<()> {
        if self.framed {
            return Ok(());
        }
        for slot in crate::shell::embeds(app)? {
            match routes::check_access(app, slot.embed.access, Some(token)).await {
                Ok(_) => {}
                Err(Error::Forbidden) => {
                    self.shell.remove(&slot.id);
                    continue;
                }
                Err(error) => return Err(error),
            }
            if self.shell.contains_key(&slot.id) {
                continue;
            }
            let service: Arc<Service> = app.services.get(slot.embed.service)?;
            let empty = HashMap::new();
            let state = service
                .0
                .mount(PageRequest {
                    operation: slot.embed.operation,
                    scope: Some(scope),
                    path: &empty,
                    query: &empty,
                    body: &[],
                    framed: false,
                })
                .await?
                .ok_or(Error::Invalid)?;
            self.generation += 1;
            let mounted = Mounted::new(
                state,
                slot.embed.access,
                self.generation,
                &slot.id,
                &self.sender,
            );
            let spec = PanelSpec {
                id: slot.id.clone(),
                embed: slot.embed_id.to_owned(),
                path: HashMap::new(),
                query: HashMap::new(),
            };
            self.shell.insert(slot.id, Panel { spec, mounted });
        }
        Ok(())
    }
    async fn update(
        &mut self,
        app: &App,
        token: &str,
        target: &str,
        input: LiveInput<'_>,
    ) -> Result<Vec<Value>> {
        let scope = self.scope(app, token).await?;
        if target == "page" {
            let mut body = self
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
            hold_flash(app, token, &mut body);
            // A page may move only among its own routes; any other path is a
            // module bug, not a navigation.
            if let Some(path) = body.path.as_deref().filter(|path| *path != self.path) {
                if self.relocate(app, token, path).await?.is_none() {
                    return Err(Error::Invalid);
                }
            }
            let mut messages = vec![self.page_message(app, &scope, body).await?];
            messages.extend(self.sync_panels(app, token, &scope).await?);
            return Ok(messages);
        }
        let panel = match self.shell.get_mut(target) {
            Some(panel) => panel,
            None => self.panels.get_mut(target).ok_or(Error::Invalid)?,
        };
        routes::check_access(app, panel.mounted.access, Some(token)).await?;
        let mut body = panel
            .mounted
            .state
            .update(LiveRequest {
                scope: &scope,
                credential: token,
                input,
            })
            .await?;
        hold_flash(app, token, &mut body);
        Ok(vec![fragment(target, body)])
    }
    async fn page_message(&mut self, app: &App, scope: &Scope, body: LiveBody) -> Result<Value> {
        let topbar_actions =
            routes::topbar_actions(&routes::resolve(app, &self.path)?.0, self.framed);
        let html = crate::shell::render(
            app,
            Some(scope),
            &self.path,
            self.framed,
            true,
            &body.html,
            topbar_actions,
        )
        .await?
        .html;
        let canonical = params::address(&self.path, &body.query);
        // A list's first render keeps a clean address: the browser's own query
        // stands while every value it carries is the page's too, and only a
        // reply whose state moved writes the canonical query (Bilimbi's
        // push_patch). A value the page dropped or corrected is rewritten, as
        // the workspace's consumed `open` is.
        let url = match self.address.take() {
            Some(address)
                if address
                    .iter()
                    .all(|(key, value)| body.query.get(key) == Some(value)) =>
            {
                Some(params::address(&self.path, &address))
            }
            Some(_) => Some(canonical.clone()),
            None => (canonical != self.canonical).then(|| canonical.clone()),
        };
        self.canonical = canonical;
        Ok(
            json!({"type":"page", "target":"app", "html":html, "url":url, "notice":body.notice, "rows":body.rows, "redirect":body.redirect, "events":events(&body.events)}),
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
                || self.shell.contains_key(&spec.id)
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
                    if crate::server_fault(&error) {
                        eprintln!("error: live panel {id} on {} failed: {error:?}", self.path);
                    }
                    self.panels.remove(&id);
                    messages.push(json!({"type":"error", "target":id, "message":error.to_string(), "clear":true}));
                }
            }
        }
        Ok(messages)
    }
    /// Perf identity for an accepted event: the page it mounts, or the page it
    /// acts on. No identity (an unroutable path, nothing mounted) is not recorded.
    fn identity(&self, app: &App, event: &Event) -> Option<String> {
        let route = match event {
            Event::Mount { path, .. } | Event::Navigate { path, .. } => {
                routes::resolve(app, path).ok()?.0.route.path.to_owned()
            }
            _ => self.route.clone(),
        };
        (!route.is_empty()).then(|| format!("event:{route}"))
    }
    async fn event(&mut self, app: &App, token: &str, event: Event) -> Result<Vec<Value>> {
        let messages = match event {
            Event::Mount { path, params } | Event::Navigate { path, params } => {
                self.mount(app, token, path, params).await?
            }
            // Back or Forward inside a `data-history="params"` page: the
            // historical path stays this page's state (its route parameters
            // join the params, after the query so the path wins) or, when it
            // names another page, mounts that page.
            Event::Params { params, path } => match path {
                Some(path) => match self.relocate(app, token, &path).await? {
                    Some(route_params) => {
                        self.address = Some(params.clone());
                        let mut params = params;
                        params.extend(route_params);
                        self.update(app, token, "page", LiveInput::Params(&params))
                            .await?
                    }
                    None => self.mount(app, token, path, params).await?,
                },
                None => {
                    self.update(app, token, "page", LiveInput::Params(&params))
                        .await?
                }
            },
            Event::Action { target, payload } => {
                self.update(app, token, &target, LiveInput::Event(&payload))
                    .await?
            }
        };
        let client = app.db.get().await?;
        let scope = app.security.sessions.activity(&**client, token).await?;
        self.bound(scope)?;
        Ok(messages)
    }
}
/// Holds the message a redirecting reply carries for the page it lands on.
fn hold_flash(app: &App, token: &str, body: &mut LiveBody) {
    if let (Some(to), Some((kind, message))) = (&body.redirect, body.flash.take()) {
        app.landing.hold(token, to, kind, message);
    }
}
fn fragment(id: &str, body: LiveBody) -> Value {
    json!({"type":"fragment", "target":id, "html":body.html, "notice":body.notice, "rows":body.rows, "redirect":body.redirect, "events":events(&body.events)})
}
/// Owner pushes for browser hooks, dispatched by live.js after the patch.
fn events(pushes: &[(&'static str, Value)]) -> Value {
    pushes
        .iter()
        .map(|(name, detail)| json!({"name":name, "detail":detail}))
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

/// Sends the browser back through the address it holds: the page's own guard
/// then decides, under the session's current identity (or none).
async fn reload(socket: &mut WebSocket, path: &str) {
    let path = if path.is_empty() { "/" } else { path };
    let message = json!({"type":"page", "target":"app", "html":"", "url":Value::Null, "notice":"", "rows":0, "redirect":path, "request_id":Value::Null, "history":"replace"});
    let _ = socket.send(Message::Text(message.to_string().into())).await;
}

pub async fn run(app: App, token: String, identity: Identity, mut socket: WebSocket, framed: bool) {
    let (sender, mut wakes) = mpsc::channel(64);
    let mut session = Session {
        identity,
        stale: false,
        framed,
        path: String::new(),
        route: String::new(),
        page_route: None,
        page: None,
        panels: HashMap::new(),
        shell: HashMap::new(),
        generation: 0,
        sender,
        canonical: String::new(),
        address: None,
    };
    // A termination of this socket's session (logout elsewhere, an operator,
    // a credential change) signs this page out at once: the browser is sent
    // back through the page it holds, whose guard forwards it to sign-in with
    // the expired-session message. A lagged receiver is covered by the next
    // guarded action or refresh, which refuses an ended session anyway.
    let mut terminations = app.security.sessions.subscribe_terminations();
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
                            let identity = session.identity(&app, &event);
                            app.perf.observe(Kind::Event, identity.as_deref(), session.event(&app, &token, event), |result| {
                                Finish::outcome(if result.is_ok() { Outcome::Ok } else { Outcome::Error })
                            }).await
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
                let mounted = if wake.target == "page" { session.page.as_ref() } else { session.shell.get(&wake.target).or_else(|| session.panels.get(&wake.target)).map(|p| &p.mounted) };
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
            _ = refresh.tick() => {
                if session.page.is_none() {continue;}
                session.update(&app, &token, "page", LiveInput::Refresh).await
            },
            terminated = terminations.recv() => {
                match terminated {
                    Ok(event) if event.id == token => {
                        reload(&mut socket, &session.path).await;
                        break;
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => continue,
                }
            },
        };
        // A stale socket's request is refused before it acted; the page is
        // reloaded rather than told of an error it did not cause.
        if session.stale {
            reload(&mut socket, &session.path).await;
            break;
        }
        let (mut messages, close) = match result {
            Ok(messages) => (messages, false),
            Err(error) => {
                if crate::server_fault(&error) {
                    eprintln!(
                        "error: live {target} event on {} failed: {error:?}",
                        session.path
                    );
                }
                let panel = target != "page" && target != "shell";
                let clear = fatal(&error);
                let parent_denied =
                    session.page.is_some() && session.scope(&app, &token).await.is_err();
                let close =
                    clear && (!panel || parent_denied || matches!(error, Error::Unauthorized));
                if panel && clear {
                    session.panels.remove(&target);
                    session.shell.remove(&target);
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
