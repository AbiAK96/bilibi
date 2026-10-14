//! Axum host: routes, cookie/Origin boundary, and one server-held page per socket.
use askama::Template;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        DefaultBodyLimit, Form, Path, Query, State, WebSocketUpgrade,
    },
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use bilibi_company::{
    web::{Detail, List},
    Company, Edit, Page, Params,
};
use bilibi_database::{Db, Error, Result};
use bilibi_security::{authorize, guard, Capability, Scope};
use bilibi_ui::{Document, Login, Shell};
use bilibi_user::Notifications;
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, sync::Arc, time::Duration};

#[derive(Clone)]
pub struct App {
    pub db: Db,
    pub notifications: Notifications,
    pub origin: Arc<str>,
    pub services: Arc<bilibi_module_registry::Services>,
    pub company: Arc<bilibi_company::CompanyService>,
    pub user: Arc<bilibi_user::UserService>,
    pub security: Arc<bilibi_security::Security>,
    pub renderer: Arc<bilibi_ui::Renderer>,
}
impl App {
    pub fn new(db: Db, origin: &str) -> Result<Self> {
        let modules = modules()?;
        let mut services = bilibi_module_registry::Services::default();
        services.provide("bootstrap.database", Arc::new(db))?;
        services.install(&modules)?;
        let db = services.get::<Db>("base/database.pool")?.as_ref().clone();
        let notifications = services
            .get::<bilibi_user::UserService>("core/user.accounts")?
            .notifications
            .clone();
        let company = services.get("core/company.companies")?;
        let user = services.get("core/user.accounts")?;
        let security = services.get("base/security.guard")?;
        let renderer = services.get("base/ui.renderer")?;
        Ok(Self {
            db,
            notifications,
            origin: origin.into(),
            company,
            user,
            security,
            renderer,
            services: Arc::new(services),
        })
    }
}

pub fn modules() -> Result<Vec<&'static bilibi_module_registry::Module>> {
    bilibi_base::link();
    bilibi_core::link();
    bilibi_domains::link();
    bilibi_extensions::link();
    Ok(bilibi_module_registry::discover()?)
}

pub async fn migrate(db: &Db) -> Result<()> {
    let modules = modules()?;
    let migrations: Vec<_> = modules
        .iter()
        .flat_map(|module| module.migrations.iter().copied())
        .collect();
    bilibi_database::migrate(db, &migrations).await
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/companies") }))
        .route("/login", get(login_page).post(login))
        .route("/logout", post(logout))
        .route("/companies", get(companies))
        .route("/companies/{id}", get(company))
        .route("/live", get(upgrade))
        .route(
            "/assets/app.css",
            get(|| async { ([(header::CONTENT_TYPE, "text/css")], bilibi_ui::CSS) }),
        )
        .route(
            "/assets/live.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], bilibi_ui::JS) }),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn(response_headers))
        .with_state(app)
}

async fn response_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (key,value) in [
        ("cache-control","no-store"),
        ("x-content-type-options","nosniff"),
        ("referrer-policy","same-origin"),
        ("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; form-action 'self'; base-uri 'none'"),
    ] { response.headers_mut().insert(axum::http::HeaderName::from_static(key),HeaderValue::from_static(value)); }
    response
}

struct HttpError(Error);
impl From<Error> for HttpError {
    fn from(error: Error) -> Self {
        Self(error)
    }
}
impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = match self.0 {
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::Forbidden => StatusCode::FORBIDDEN,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Invalid => StatusCode::BAD_REQUEST,
            Error::Conflict => StatusCode::CONFLICT,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        (status, self.0.to_string()).into_response()
    }
}
fn render<T: Template>(template: T) -> Result<String> {
    template.render().map_err(|_| Error::Internal)
}
fn document(body: &str) -> Result<Html<String>> {
    Ok(Html(render(Document { body })?))
}
fn token(headers: &HeaderMap) -> Result<String> {
    headers
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| {
            h.split(';').find_map(|cookie| {
                cookie
                    .trim()
                    .strip_prefix("bilibi_session=")
                    .map(str::to_owned)
            })
        })
        .filter(|s| s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or(Error::Unauthorized)
}
fn origin(app: &App, headers: &HeaderMap) -> Result<()> {
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(&app.origin) {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}
async fn login_page() -> std::result::Result<Html<String>, HttpError> {
    Ok(document(&render(Login { error: "" })?)?)
}
#[derive(Deserialize)]
struct Credentials {
    tenant: i64,
    email: String,
    password: String,
}
async fn login(
    State(app): State<App>,
    headers: HeaderMap,
    Form(form): Form<Credentials>,
) -> std::result::Result<Response, HttpError> {
    origin(&app, &headers)?;
    match app
        .user
        .login(form.tenant, &form.email, &form.password)
        .await
    {
        Ok(session) => {
            let secure = if app.origin.starts_with("https://") {
                "; Secure"
            } else {
                ""
            };
            let cookie = format!(
                "bilibi_session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=86400{secure}",
                session.token
            );
            let mut response = Redirect::to("/companies").into_response();
            response.headers_mut().insert(
                header::SET_COOKIE,
                HeaderValue::from_str(&cookie).map_err(|_| Error::Internal)?,
            );
            Ok(response)
        }
        Err(Error::Unauthorized) => Ok((
            StatusCode::UNAUTHORIZED,
            document(&render(Login {
                error: "Tenant, email or password is incorrect",
            })?)?,
        )
            .into_response()),
        Err(error) => Err(error.into()),
    }
}
#[derive(Deserialize)]
struct Logout {
    csrf: String,
}
async fn logout(
    State(app): State<App>,
    headers: HeaderMap,
    Form(form): Form<Logout>,
) -> std::result::Result<Response, HttpError> {
    origin(&app, &headers)?;
    app.user.logout(&token(&headers)?, &form.csrf).await?;
    let mut response = Redirect::to("/login").into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("bilibi_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"),
    );
    Ok(response)
}

#[derive(Clone, Debug)]
enum Route {
    List,
    Detail(i64),
}
impl Route {
    fn parse(path: &str) -> Result<Self> {
        if path == "/companies" {
            return Ok(Self::List);
        }
        let id = path
            .strip_prefix("/companies/")
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .ok_or(Error::Invalid)?;
        Ok(Self::Detail(id))
    }
    fn path(&self) -> String {
        match self {
            Self::List => "/companies".into(),
            Self::Detail(id) => format!("/companies/{id}"),
        }
    }
}
fn params(values: &HashMap<String, String>) -> Result<Params> {
    let mut result = Params::default();
    for (key, value) in values {
        match key.as_str() {
            "page" => result.page = value.parse().map_err(|_| Error::Invalid)?,
            "size" => result.size = value.parse().map_err(|_| Error::Invalid)?,
            "sort" => result.sort = value.clone(),
            "dir" => result.dir = value.clone(),
            "q" => result.q = value.clone(),
            "status" => result.status = value.clone(),
            _ => return Err(Error::Invalid),
        }
    }
    result.validate()?;
    Ok(result)
}

// Snapshot belongs to this socket, including loaded redacted records and current
// layout. Every operation replaces it from fresh scope, grants, and DB rows.
enum Content {
    List(Page),
    Detail(Company),
}
struct Loaded {
    html: String,
    scope: Scope,
    content: Content,
}
impl Loaded {
    fn row_count(&self) -> usize {
        match &self.content {
            Content::List(page) => page.rows.len(),
            Content::Detail(company) => usize::from(company.id > 0),
        }
    }
}
async fn load(app: &App, token: &str, route: &Route, params: &Params) -> Result<Loaded> {
    let client = app.db.get().await?;
    let scope = guard(&**client, token).await?;
    let (body, content) = match route {
        Route::List => {
            let page = bilibi_company::list(&**client, &scope, params).await?;
            let pages = ((page.total + params.size - 1) / params.size).max(1);
            (
                app.renderer
                    .render(List {
                        page: &page,
                        params,
                        pages,
                    })
                    .map_err(|_| Error::Internal)?,
                Content::List(page),
            )
        }
        Route::Detail(id) => {
            let (company, masked) = bilibi_company::detail(&**client, &scope, *id).await?;
            let editable = match authorize(&**client, &scope, Capability::CompaniesUpdate).await {
                Ok(()) => true,
                Err(Error::Forbidden) => false,
                Err(error) => return Err(error),
            };
            (
                app.renderer
                    .render(Detail::new(&company, &masked, editable))
                    .map_err(|_| Error::Internal)?,
                Content::Detail(company),
            )
        }
    };
    let unread = bilibi_user::unread(&**client, &scope).await?;
    let html = app
        .renderer
        .render(Shell {
            user: &scope.user_name,
            email: &scope.email,
            tenant: &scope.tenant_name,
            csrf: &scope.csrf,
            unread,
            body: &body,
        })
        .map_err(|_| Error::Internal)?;
    Ok(Loaded {
        html,
        scope,
        content,
    })
}
async fn http_page(
    app: App,
    headers: HeaderMap,
    route: Route,
    values: HashMap<String, String>,
) -> std::result::Result<Response, HttpError> {
    let token = match token(&headers) {
        Ok(token) => token,
        Err(_) => return Ok(Redirect::to("/login").into_response()),
    };
    let loaded = load(&app, &token, &route, &params(&values)?).await?;
    Ok(document(&loaded.html)?.into_response())
}
async fn companies(
    State(app): State<App>,
    headers: HeaderMap,
    Query(values): Query<HashMap<String, String>>,
) -> std::result::Result<Response, HttpError> {
    http_page(app, headers, Route::List, values).await
}
async fn company(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> std::result::Result<Response, HttpError> {
    http_page(app, headers, Route::Detail(id), HashMap::new()).await
}
async fn upgrade(
    State(app): State<App>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> std::result::Result<Response, HttpError> {
    origin(&app, &headers)?;
    let token = token(&headers)?;
    let client = app.db.get().await.map_err(Error::from)?;
    let scope = app.security.guard(&token).await?;
    authorize(&**client, &scope, Capability::CompaniesView).await?;
    Ok(ws
        .max_message_size(16 * 1024)
        .max_frame_size(16 * 1024)
        .on_upgrade(move |socket| live(app, token, socket)))
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Mount {
        path: String,
        #[serde(default)]
        params: HashMap<String, String>,
    },
    Params {
        params: HashMap<String, String>,
    },
    Sort {
        field: String,
    },
    Page {
        delta: i64,
    },
    Save {
        edit: Edit,
    },
    Read,
}
struct LivePage {
    route: Route,
    params: Params,
    loaded: Option<Loaded>,
}
impl Default for LivePage {
    fn default() -> Self {
        Self {
            route: Route::List,
            params: Params::default(),
            loaded: None,
        }
    }
}
impl LivePage {
    async fn event(&mut self, app: &App, token: &str, event: Event) -> Result<&'static str> {
        let client = app.db.get().await?;
        let scope = guard(&**client, token).await?;
        authorize(&**client, &scope, Capability::CompaniesView).await?;
        drop(client);
        let mut notice = "";
        match event {
            Event::Mount {
                path,
                params: values,
            } => {
                self.loaded = None;
                self.route = Route::parse(&path)?;
                self.params = params(&values)?;
            }
            _ if self.loaded.is_none() => return Err(Error::Invalid),
            Event::Params { params: values } => self.params = params(&values)?,
            Event::Sort { field } => {
                if !["name", "status", "created_at"].contains(&field.as_str()) {
                    return Err(Error::Invalid);
                }
                self.params.dir = if self.params.sort == field && self.params.dir == "asc" {
                    "desc".into()
                } else {
                    "asc".into()
                };
                self.params.sort = field;
                self.params.page = 1;
            }
            Event::Page { delta } => {
                if ![-1, 1].contains(&delta) {
                    return Err(Error::Invalid);
                }
                self.params.page = (self.params.page + delta).max(1);
                self.params.validate()?;
            }
            Event::Save { edit } => {
                if !matches!(self.route,Route::Detail(id) if id==edit.id) {
                    return Err(Error::Invalid);
                }
                app.company.save(token, &edit).await?;
                notice = "Saved · audit recorded";
            }
            Event::Read => {
                app.user.mark_read(token).await?;
                notice = "Notifications marked read";
            }
        }
        self.loaded = Some(load(app, token, &self.route, &self.params).await?);
        Ok(notice)
    }
    fn message(&self, notice: &str) -> serde_json::Value {
        let loaded = self.loaded.as_ref().expect("loaded after successful event");
        let query = if matches!(self.route, Route::List) {
            // URL values come from typed state; encode query values, never HTML.
            format!(
                "?page={}&size={}&sort={}&dir={}&q={}&status={}",
                self.params.page,
                self.params.size,
                self.params.sort,
                self.params.dir,
                encode(&self.params.q),
                self.params.status
            )
        } else {
            String::new()
        };
        json!({"type":"page","html":loaded.html,"url":format!("{}{query}",self.route.path()),"notice":notice,"rows":loaded.row_count()})
    }
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

async fn live(app: App, token: String, mut socket: WebSocket) {
    let mut page = LivePage::default();
    let mut events = app.notifications.subscribe();
    let mut refresh = tokio::time::interval(Duration::from_secs(15));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    refresh.tick().await;
    loop {
        let result = tokio::select! {
            message=socket.recv()=>match message {
                Some(Ok(Message::Text(text)))=>match serde_json::from_str::<Event>(&text) { Ok(event)=>page.event(&app,&token,event).await,Err(_)=>Err(Error::Invalid) },
                Some(Ok(Message::Ping(data)))=>{if socket.send(Message::Pong(data)).await.is_err(){break;}continue;},
                Some(Ok(Message::Pong(_)))=>continue,
                _=>break,
            },
            notification=events.recv()=>{
                match notification {
                    Ok(event) => if !page.loaded.as_ref().is_some_and(|loaded|loaded.scope.tenant()==event.tenant && loaded.scope.user()==event.user) {continue;},
                    Err(tokio::sync::broadcast::error::RecvError::Closed)=>break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>{},
                }
                if page.loaded.is_none() {continue;}
                page.loaded=None;
                match load(&app,&token,&page.route,&page.params).await {Ok(loaded)=>{page.loaded=Some(loaded);Ok("")},Err(error)=>Err(error)}
            },
            _=refresh.tick()=>{
                if page.loaded.is_none(){continue;}
                page.loaded=None;
                match load(&app,&token,&page.route,&page.params).await {Ok(loaded)=>{page.loaded=Some(loaded);Ok("")},Err(error)=>Err(error)}
            },
        };
        let (message, clear) = match result {
            Ok(notice) => (page.message(notice), false),
            Err(error) => {
                let clear = matches!(
                    error,
                    Error::Unauthorized
                        | Error::Forbidden
                        | Error::NotFound
                        | Error::Database(_)
                        | Error::Pool(_)
                        | Error::Internal
                );
                if clear {
                    page.loaded = None;
                }
                (
                    json!({"type":"error","message":error.to_string(),"clear":clear}),
                    clear,
                )
            }
        };
        if socket
            .send(Message::Text(message.to_string().into()))
            .await
            .is_err()
            || clear
        {
            break;
        }
    }
}

pub mod seed;
