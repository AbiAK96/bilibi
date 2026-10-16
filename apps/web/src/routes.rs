//! Discovered HTTP pages: validate/bind once, authorize before owner dispatch.
use crate::{origin, token, App, HttpError};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{on, MethodFilter},
    Router,
};
use bilibi_database::{Error, Result};
use bilibi_module_registry::{
    contributions::{Access, Method, Route, ROUTES},
    RegistryError, Services,
};
use bilibi_ui::page::PageRequest;
use std::{collections::HashMap, sync::Arc};

type PageService = bilibi_ui::page::PageService<bilibi_security::Scope, Error>;

#[derive(Clone)]
pub struct BoundRoute {
    pub route: &'static Route,
    pub page: Arc<PageService>,
}

/// Router-owned paths cannot be shadowed, even by another method. Literal
/// siblings beat parameters; crossed patterns and parameter renames fail boot.
pub fn bind(services: &Services) -> Result<Vec<BoundRoute>> {
    let mut bound = Vec::new();
    for entry in services.catalog()?.entries::<Route>(ROUTES)? {
        let route = entry.get::<Route>()?;
        validate_pattern(route.path)?;
        if crate::api::reserved(route.path) {
            return Err(invalid(format!(
                "{} at {}:{} claims the host agent path {}",
                entry.id, entry.provenance.source, entry.provenance.line, route.path
            )));
        }
        for reserved in [
            "/",
            "/login",
            "/logout",
            "/preferences",
            "/forgot-password",
            "/reset-password/{token}",
            "/live",
            "/fonts/{name}",
            "/favicon.svg",
            "/favicon.ico",
            "/assets/{name}",
            crate::media::STREAM,
            crate::media::ATTACHMENT,
            crate::pdf::RENDER,
            crate::webhooks::PATH,
            crate::impersonation::ENTER,
            crate::impersonation::LEAVE,
        ] {
            if intersects(route.path, reserved) {
                return Err(invalid(format!(
                    "{} at {}:{} overlaps host path {reserved}",
                    entry.id, entry.provenance.source, entry.provenance.line
                )));
            }
        }
        for previous in &bound {
            validate_pair(previous, route)?;
        }
        bound.push(BoundRoute {
            route,
            page: services.get(route.service)?,
        });
    }
    Ok(bound)
}
fn invalid(message: String) -> Error {
    RegistryError::Contribution(message).into()
}
fn parameter(segment: &str) -> bool {
    segment.starts_with('{')
}
fn validate_pattern(path: &str) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    if !path.starts_with('/')
        || path.contains(['?', '#', '%', '\\'])
        || path.contains("//")
        || (path.len() > 1 && path.ends_with('/'))
    {
        return Err(invalid(format!("invalid page path {path}")));
    }
    for segment in path.split('/').skip(1) {
        if parameter(segment) {
            let name = segment
                .strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .unwrap_or("");
            if name.is_empty()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || !names.insert(name)
            {
                return Err(invalid(format!("invalid page parameter in {path}")));
            }
        } else if segment.contains(['{', '}', '*', ':']) || [".", ".."].contains(&segment) {
            return Err(invalid(format!("invalid page segment in {path}")));
        }
    }
    Ok(())
}
fn intersects(a: &str, b: &str) -> bool {
    let a: Vec<_> = a.split('/').collect();
    let b: Vec<_> = b.split('/').collect();
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a == &b || parameter(a) || parameter(b))
}
fn validate_pair(previous: &BoundRoute, route: &Route) -> Result<()> {
    let other = previous.route;
    // Axum shares parameter nodes even for disjoint methods and suffixes.
    for (a, b) in other.path.split('/').zip(route.path.split('/')) {
        if parameter(a) && parameter(b) && a != b {
            return Err(invalid(format!(
                "conflicting parameter names: {} and {}",
                other.path, route.path
            )));
        }
        if a != b && !(parameter(a) && parameter(b)) {
            break;
        }
    }
    if other.method != route.method || !intersects(other.path, route.path) {
        return Ok(());
    }
    let mut left = false;
    let mut right = false;
    for (a, b) in other.path.split('/').zip(route.path.split('/')) {
        left |= !parameter(a) && parameter(b);
        right |= parameter(a) && !parameter(b);
    }
    if left == right {
        return Err(invalid(format!(
            "ambiguous page routes: {} and {}",
            other.path, route.path
        )));
    }
    Ok(())
}

pub fn install(mut router: Router<App>, routes: &[BoundRoute]) -> Router<App> {
    for bound in routes {
        let bound = bound.clone();
        let method = match bound.route.method {
            Method::Get => MethodFilter::GET,
            Method::Post => MethodFilter::POST,
            Method::Put => MethodFilter::PUT,
            Method::Patch => MethodFilter::PATCH,
            Method::Delete => MethodFilter::DELETE,
        };
        router = router.route(
            bound.route.path,
            on(
                method,
                move |State(app): State<App>,
                      uri: Uri,
                      headers: HeaderMap,
                      Path(path): Path<HashMap<String, String>>,
                      Query(query): Query<HashMap<String, String>>,
                      body: Bytes| {
                    dispatch(app, bound, uri, headers, path, query, body)
                },
            ),
        );
    }
    router
}

pub async fn check_access(
    app: &App,
    access: Access,
    credential: Option<&str>,
) -> Result<Option<bilibi_security::Scope>> {
    if matches!(access, Access::Public) {
        return Ok(None);
    }
    let scope = app
        .security
        .guard(credential.ok_or(Error::Unauthorized)?)
        .await?;
    let client = app.db.get().await.map_err(Error::from)?;
    match access {
        Access::Capability(key) => bilibi_authz::authorize_key(&**client, &scope, key).await?,
        Access::Operator => {
            bilibi_tenancy::prove_operator(&**client, &scope).await?;
        }
        Access::OperatorCapability(key) => {
            bilibi_authz::authorize_operator(&**client, &scope, key).await?;
        }
        Access::SignedIn => {}
        Access::Public => unreachable!(),
    }
    Ok(Some(scope))
}

async fn dispatch(
    app: App,
    bound: BoundRoute,
    uri: Uri,
    headers: HeaderMap,
    path: HashMap<String, String>,
    query: HashMap<String, String>,
    body: Bytes,
) -> std::result::Result<Response, HttpError> {
    let credential = token(&headers).ok();
    let scope = match check_access(&app, bound.route.access, credential.as_deref()).await {
        Err(Error::Unauthorized) if bound.route.method == Method::Get => {
            return Ok(crate::auth::sign_in_redirect(&app, credential.is_some()))
        }
        result => result?,
    };
    if bound.route.method != Method::Get {
        origin(&app, &headers)?;
        if let Some(scope) = &scope {
            if headers.get("x-csrf-token").and_then(|h| h.to_str().ok())
                != Some(scope.csrf.as_str())
            {
                return Err(Error::Forbidden.into());
            }
        }
    }
    let framed = matches!(
        headers.get("sec-fetch-dest").and_then(|h| h.to_str().ok()),
        Some("iframe" | "frame")
    );
    if framed && !bound.route.framed {
        return Err(Error::Forbidden.into());
    }
    let rendered = bound
        .page
        .0
        .render(PageRequest {
            operation: bound.route.operation,
            scope: scope.as_ref(),
            path: &path,
            query: &query,
            body: &body,
            framed,
        })
        .await;
    let page = match rendered {
        Err(Error::NotFound) if bound.route.method == Method::Get => {
            return match crate::auth::missing_record_redirect(&app, bound.route.path) {
                Some(response) => Ok(response),
                None => Err(Error::NotFound.into()),
            }
        }
        page => page?,
    };
    if let Some(destination) = bound.page.0.redirect(bound.route.operation) {
        return Ok(axum::response::Redirect::to(destination).into_response());
    }
    let shell = crate::shell::render(
        &app,
        scope.as_ref(),
        uri.path(),
        framed,
        page.live,
        &page.html,
        topbar_actions(&bound, framed),
    )
    .await?;
    if let (Some(credential), Some(_)) = (credential, scope) {
        let client = app.db.get().await.map_err(Error::from)?;
        app.security
            .sessions
            .activity(&**client, &credential)
            .await?;
    }
    let title = page
        .title
        .as_deref()
        .or_else(|| bound.page.0.title(bound.route.operation))
        .unwrap_or("Bilibi");
    // A missing record elsewhere sent the visitor here with its message.
    let waiting = crate::auth::waiting_flash(&headers).filter(|_| !framed);
    let html = match waiting {
        Some(message) => format!(
            "{}{}",
            shell.html,
            bilibi_ui::Html::render(&bilibi_ui::FlashGroup {
                id: "flash-redirect",
                flash: bilibi_ui::FlashMap {
                    error: message,
                    ..bilibi_ui::FlashMap::default()
                },
            })
            .map_err(|_| Error::Internal)?
        ),
        None => shell.html,
    };
    let mut response = crate::document_themed(title, &html, shell.theme)?.into_response();
    if waiting.is_some() {
        crate::auth::clear_flash(&mut response);
    }
    if framed {
        response.headers_mut().insert(
            header::HeaderName::from_static("x-bilibi-framed"),
            header::HeaderValue::from_static("true"),
        );
    }
    *response.status_mut() = StatusCode::OK;
    Ok(response)
}

/// Resolve the same discovered GET routes as HTTP, with literal siblings first.
pub fn resolve(app: &App, path: &str) -> Result<(BoundRoute, HashMap<String, String>)> {
    if !path.starts_with('/')
        || path.len() > 2048
        || path.contains(['?', '#', '\\'])
        || path.contains("//")
    {
        return Err(Error::Invalid);
    }
    let segments: Vec<_> = path.split('/').map(decode_segment).collect::<Result<_>>()?;
    let mut matches = Vec::new();
    for bound in app.routes.iter().filter(|b| b.route.method == Method::Get) {
        let pattern: Vec<_> = bound.route.path.split('/').collect();
        if pattern.len() != segments.len() {
            continue;
        }
        let mut params = HashMap::new();
        let mut specificity = 0;
        let mut valid = true;
        for (pattern, value) in pattern.into_iter().zip(&segments) {
            if parameter(pattern) {
                if value.is_empty() || [".", ".."].contains(&value.as_str()) {
                    valid = false;
                    break;
                }
                params.insert(pattern[1..pattern.len() - 1].into(), value.clone());
            } else if pattern == value {
                specificity += 1;
            } else {
                valid = false;
                break;
            }
        }
        if valid {
            matches.push((specificity, bound.clone(), params));
        }
    }
    matches
        .into_iter()
        .max_by_key(|(score, _, _)| *score)
        .map(|(_, bound, params)| (bound, params))
        .ok_or(Error::NotFound)
}

fn decode_segment(segment: &str) -> Result<String> {
    let mut bytes = Vec::with_capacity(segment.len());
    let mut input = segment.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = input
                .next()
                .and_then(|v| (v as char).to_digit(16))
                .ok_or(Error::Invalid)?;
            let low = input
                .next()
                .and_then(|v| (v as char).to_digit(16))
                .ok_or(Error::Invalid)?;
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    let value = String::from_utf8(bytes).map_err(|_| Error::Invalid)?;
    if value.contains(['/', '\\']) || value.chars().any(char::is_control) {
        return Err(Error::Invalid);
    }
    Ok(value)
}

/// The page's top-bar controls for the shell; a framed render has no top bar.
pub fn topbar_actions(bound: &BoundRoute, framed: bool) -> bilibi_ui::Html {
    if framed {
        return bilibi_ui::Html::default();
    }
    bound
        .page
        .0
        .topbar_actions(bound.route.operation)
        .unwrap_or_default()
}
