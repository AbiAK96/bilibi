//! Axum host: routes, cookie/Origin boundary, and one server-held page per socket.
mod live;
mod media;
mod pdf;
mod routes;
use askama::Template;
use axum::{
    extract::{DefaultBodyLimit, Form, Path, Query, State, WebSocketUpgrade},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use bilibi_database::{Db, Error, Result};
use bilibi_ui::{Document, Login};
use bilibi_user::Notifications;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone)]
pub struct App {
    routes: Arc<Vec<routes::BoundRoute>>,
    pub db: Db,
    pub notifications: Notifications,
    pub origin: Arc<str>,
    pub services: Arc<bilibi_module_registry::Services>,
    pub company: Arc<bilibi_company::CompanyService>,
    pub user: Arc<bilibi_user::UserService>,
    pub security: Arc<bilibi_security::Security>,
    pub renderer: Arc<bilibi_ui::Renderer>,
    pub menu: Arc<bilibi_base::menu::Menu>,
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
        let menu = bilibi_base::menu::Menu::from_catalog(services.catalog()?)?;
        for dropped in menu.dropped() {
            eprintln!("warning: {dropped}");
        }
        let routes = Arc::new(routes::bind(&services)?);
        Ok(Self {
            routes,
            db,
            notifications,
            origin: origin.into(),
            company,
            user,
            security,
            renderer,
            menu: Arc::new(menu),
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

/// `media-cleanup` command: one tenant's abandoned private attachment drafts.
pub async fn media_cleanup(db: &Db, tenant: i64, hours: u64) -> Result<()> {
    media::cleanup(db, tenant, hours).await
}

pub fn router(app: App) -> Router {
    routes::install(Router::new(), &app.routes)
        .route("/", get(|| async { Redirect::to("/companies") }))
        .route("/login", get(login_page).post(login))
        .route("/logout", post(logout))
        .route("/live", get(upgrade))
        .route(media::STREAM, get(media::stream))
        .route(media::ATTACHMENT, get(media::attachment))
        .route(pdf::RENDER, get(pdf::render))
        .route("/fonts/{name}", get(font_asset))
        .route(
            "/assets/ui-specimen.html",
            get(|| async { asset_response("ui-specimen.html") }),
        )
        .route("/assets/ui-components.html", get(components_specimen))
        .route("/assets/ui-feedback.html", get(feedback_specimen))
        .route(
            "/assets/ui-feedback-demo.js",
            get(|| async { asset_response("ui-feedback-demo.js") }),
        )
        .route(
            "/assets/ui.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript")],
                    bilibi_ui::UI_JS,
                )
            }),
        )
        .route(
            "/favicon.svg",
            get(|| async { asset_response("favicon.svg") }),
        )
        .route(
            "/favicon.ico",
            get(|| async { asset_response("favicon.ico") }),
        )
        .route(
            "/assets/app.css",
            get(|| async { ([(header::CONTENT_TYPE, "text/css")], bilibi_ui::CSS) }),
        )
        .route(
            "/assets/hooks.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript")],
                    bilibi_ui::HOOKS_JS,
                )
            }),
        )
        .route(
            "/assets/live.js",
            get(|| async {
                let script = bilibi_ui::JS.replace(
                    "__BILIBI_DEV_RELOAD__",
                    if cfg!(debug_assertions) {
                        "true"
                    } else {
                        "false"
                    },
                );
                ([(header::CONTENT_TYPE, "text/javascript")], script)
            }),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn(response_headers))
        .with_state(app)
}

/// The shared component specimen: the screenshot baseline for the primitives
/// in `bilibi_ui`, served like the asset specimen and not a product page.
async fn components_specimen() -> std::result::Result<Html<String>, HttpError> {
    let specimen = bilibi_ui::ComponentsSpecimen::new().map_err(|_| Error::Internal)?;
    Ok(Html(render(specimen)?))
}

/// The feedback, dialog and tab specimen: the screenshot baseline for those
/// components, served beside the primitives specimen.
async fn feedback_specimen() -> std::result::Result<Html<String>, HttpError> {
    Ok(Html(render(bilibi_ui::FeedbackSpecimen::new())?))
}

fn asset_response(name: &str) -> Response {
    match bilibi_ui::static_asset(name) {
        Some(asset) => ([(header::CONTENT_TYPE, asset.content_type)], asset.bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn font_asset(Path(name): Path<String>) -> Response {
    asset_response(&name)
}

/// Defaults only: a handler that sets its own policy, as signed media streams
/// do with a sandboxing CSP, keeps it.
async fn response_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (key,value) in [
        ("cache-control","no-store"),
        ("x-content-type-options","nosniff"),
        ("referrer-policy","same-origin"),
        ("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'self'; form-action 'self'; base-uri 'none'"),
    ] { response.headers_mut().entry(axum::http::HeaderName::from_static(key)).or_insert(HeaderValue::from_static(value)); }
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

async fn upgrade(
    State(app): State<App>,
    headers: HeaderMap,
    Query(options): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> std::result::Result<Response, HttpError> {
    origin(&app, &headers)?;
    let token = token(&headers)?;
    // Upgrade proves the session only. Each discovered page/panel has its own gate.
    app.security.guard(&token).await?;
    Ok(ws
        .max_message_size(16 * 1024)
        .max_frame_size(16 * 1024)
        .on_upgrade(move |socket| {
            live::run(
                app,
                token,
                socket,
                options.get("framed").is_some_and(|v| v == "true"),
            )
        }))
}

pub mod seed;
