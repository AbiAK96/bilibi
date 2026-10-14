//! Discovered HTTP pages: validate/bind once, authorize before owner dispatch.
use crate::{document, origin, token, App, HttpError};
use askama::Template;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{on, MethodFilter},
    Router,
};
use bilibi_database::{Error, Result};
use bilibi_module_registry::{
    contributions::{Access, Method, Route, ROUTES},
    RegistryError, Services,
};
use bilibi_ui::{page::PageRequest, Framed, Shell};
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
        for reserved in [
            "/",
            "/login",
            "/logout",
            "/live",
            "/fonts/{name}",
            "/favicon.svg",
            "/favicon.ico",
            "/assets/{name}",
            crate::media::STREAM,
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
                      headers: HeaderMap,
                      Path(path): Path<HashMap<String, String>>,
                      Query(query): Query<HashMap<String, String>>,
                      body: Bytes| dispatch(app, bound, headers, path, query, body),
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
    headers: HeaderMap,
    path: HashMap<String, String>,
    query: HashMap<String, String>,
    body: Bytes,
) -> std::result::Result<Response, HttpError> {
    let credential = token(&headers).ok();
    let scope = match check_access(&app, bound.route.access, credential.as_deref()).await {
        Err(Error::Unauthorized) if bound.route.method == Method::Get => {
            return Ok(Redirect::to("/login").into_response())
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
    let page = bound
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
        .await?;
    let html = if framed {
        Framed {
            body: &page.html,
            live: page.live,
        }
        .render()
        .map_err(|_| Error::Internal)?
    } else if let Some(scope) = &scope {
        let client = app.db.get().await.map_err(Error::from)?;
        let unread = match bilibi_user::unread(&**client, scope).await {
            Ok(unread) => unread,
            Err(Error::Forbidden) => 0,
            Err(error) => return Err(error.into()),
        };
        Shell {
            user: &scope.user_name,
            email: &scope.email,
            tenant: &scope.tenant_name,
            csrf: &scope.csrf,
            unread,
            live: page.live,
            body: &page.html,
        }
        .render()
        .map_err(|_| Error::Internal)?
    } else {
        page.html
    };
    if let (Some(credential), Some(_)) = (credential, scope) {
        let client = app.db.get().await.map_err(Error::from)?;
        app.security
            .sessions
            .activity(&**client, &credential)
            .await?;
    }
    let mut response = document(&html)?.into_response();
    if framed {
        response.headers_mut().insert(
            header::HeaderName::from_static("x-bilibi-framed"),
            header::HeaderValue::from_static("true"),
        );
    }
    *response.status_mut() = StatusCode::OK;
    Ok(response)
}
