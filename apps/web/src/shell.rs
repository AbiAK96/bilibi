//! The authenticated chrome around a discovered page (`bilibi_ui::AppShell`,
//! `Framed`) and the shell's own preference write. Web decides what the shell
//! shows from the sealed scope: the menu through the actor's effective
//! capabilities, operator standing through the explicit proof, the clock
//! through the actor's display policy. Base UI only renders it.
use crate::{origin, token, App, HttpError};
use askama::Template;
use axum::{
    body::Bytes,
    extract::State,
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
};
use bilibi_base::{authz, locale, menu, tenancy};
use bilibi_database::{Error, Result};
use bilibi_module_registry::contributions::Method;
use bilibi_security::Scope;
use bilibi_ui::{AppShell, DisplayPreferences, Framed, Html, NavNode, ShellContext};
use serde::Deserialize;
use serde_json::json;

/// Renders `body` inside the full shell, the chromeless framed root, or as is
/// for a public page outside a frame.
pub async fn render(
    app: &App,
    scope: Option<&Scope>,
    path: &str,
    framed: bool,
    live: bool,
    body: &str,
) -> Result<String> {
    let preferences = match scope {
        Some(scope) => preferences(app, scope).await?,
        None => DisplayPreferences::default(),
    };
    let html = if framed {
        Framed {
            body,
            live,
            tenant_id: scope.map_or(0, Scope::tenant),
            user_id: scope.map_or(0, Scope::user),
            preferences: &preferences,
        }
        .render()
    } else if let Some(scope) = scope {
        let context = context(app, scope, path, preferences).await?;
        AppShell {
            context: &context,
            live,
            body,
        }
        .render()
    } else {
        return Ok(body.to_owned());
    };
    html.map_err(|_| Error::Internal)
}

/// The time display is the actor's saved mode over the company zone. The
/// theme stays the browser's choice until C5 persists appearance.
async fn preferences(app: &App, scope: &Scope) -> Result<DisplayPreferences> {
    let client = app.db.get().await?;
    let display = app
        .display
        .display(&**client, scope, None, locale::FALLBACK)
        .await?;
    Ok(DisplayPreferences {
        mode: display.mode.as_str().into(),
        timezone: display.timezone,
        ..DisplayPreferences::default()
    })
}

async fn context<'a>(
    app: &App,
    scope: &'a Scope,
    path: &str,
    preferences: DisplayPreferences,
) -> Result<ShellContext<'a>> {
    let client = app.db.get().await?;
    let capabilities = authz::effective_capabilities(&**client, scope).await?;
    let tree = app.menu.visible_tree(&|key| capabilities.contains(key));
    let active = active_item(&tree, path);
    let nodes: Vec<_> = tree.iter().map(|node| nav_node(node, active)).collect();
    // The tenant's standing, from the explicit operator proof only.
    let platform_operator = match tenancy::prove_operator(&**client, scope).await {
        Ok(_) => true,
        Err(Error::Forbidden) => false,
        Err(error) => return Err(error),
    };
    // Core User contributes the bell; an account that may not view
    // notifications has none.
    let notifications = match bilibi_user::unread(&**client, scope).await {
        Ok(unread) => bilibi_user::web::bell(unread).map_err(|_| Error::Internal)?,
        Err(Error::Forbidden) => Html::default(),
        Err(error) => return Err(error),
    };
    let served: Vec<_> = app
        .routes
        .iter()
        .filter(|bound| bound.route.method == Method::Get)
        .map(|bound| bound.route.path)
        .collect();
    let home = if served.contains(&"/dashboard") {
        "/dashboard"
    } else {
        "/"
    };
    Ok(ShellContext {
        user: &scope.user_name,
        email: &scope.email,
        tenant: &scope.tenant_name,
        tenant_id: scope.tenant(),
        user_id: scope.user(),
        // K17 binds the affiliation's company label.
        company: None,
        platform_operator,
        csrf: &scope.csrf,
        preferences,
        navigation: bilibi_ui::navigation(&nodes, true).map_err(|_| Error::Internal)?,
        notifications,
        home,
        served_routes: serde_json::to_string(&served).map_err(|_| Error::Internal)?,
        development: cfg!(debug_assertions),
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// The page's own menu row: an exact route, else the deepest route this path
/// sits under (a company's detail page keeps Companies current).
fn active_item(tree: &[menu::Node], path: &str) -> Option<&'static str> {
    fn walk(nodes: &[menu::Node], path: &str, best: &mut Option<(&'static str, usize)>) {
        for node in nodes {
            let item = node.item.item;
            if let Some(route) = item.route {
                let under = route == path
                    || route == "/"
                    || path
                        .strip_prefix(route)
                        .is_some_and(|rest| rest.starts_with('/'));
                if under && best.is_none_or(|(_, len)| route.len() > len) {
                    *best = Some((item.id, route.len()));
                }
            }
            walk(&node.children, path, best);
        }
    }
    let mut best = None;
    walk(tree, path, &mut best);
    best.map(|(id, _)| id)
}

fn nav_node(node: &menu::Node, active: Option<&str>) -> NavNode {
    let item = node.item.item;
    // Contributions carry bare Heroicon names; the host adds the set prefix.
    let icon = match item.icon {
        None => "hero-square-3-stack-3d".to_owned(),
        Some(name) if name.starts_with("hero-") => name.to_owned(),
        Some(name) => format!("hero-{name}"),
    };
    NavNode {
        id: item.id.to_owned(),
        label: item.label.to_owned(),
        icon,
        route: item.route.map(str::to_owned),
        active: active == Some(item.id),
        children: node
            .children
            .iter()
            .map(|child| nav_node(child, active))
            .collect(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preference {
    kind: String,
    value: String,
}

/// `POST /preferences`: the shell's time-display chooser. Same session,
/// Origin and CSRF checks as a page write; the actor can only change its own
/// mode. Replies with the saved mode.
pub async fn save(
    State(app): State<App>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Response, HttpError> {
    origin(&app, &headers)?;
    let credential = token(&headers)?;
    let scope = app.security.guard(&credential).await?;
    if headers.get("x-csrf-token").and_then(|h| h.to_str().ok()) != Some(scope.csrf.as_str()) {
        return Err(Error::Forbidden.into());
    }
    let preference: Preference = serde_json::from_slice(&body).map_err(|_| Error::Invalid)?;
    if preference.kind != "timezone" {
        return Err(Error::Invalid.into());
    }
    let mut client = app.db.get().await.map_err(Error::from)?;
    let mut tx = app
        .mutations
        .begin(
            &mut client,
            scope.mutation_context("shell.display_preference"),
        )
        .await?;
    let mode = app
        .display
        .put_mode(&mut tx, &scope, &preference.value)
        .await?;
    tx.commit().await?;
    app.security
        .sessions
        .activity(&**client, &credential)
        .await?;
    Ok((
        [(header::CONTENT_TYPE, "application/json")],
        json!({"mode": mode.as_str()}).to_string(),
    )
        .into_response())
}
