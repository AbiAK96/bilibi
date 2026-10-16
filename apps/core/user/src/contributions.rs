//! The shell consumes a contributed bell; Base UI never imports Core User.
//! `/notifications` is the bell's full list.
//! The account's self-service pages are discovered routes served by
//! `core/user.pages`; Web establishes the session before dispatching.
use bilibi_authz::{CapabilityDefinition as C, RoleDefinition, VocabularyContribution, VOCABULARY};
use bilibi_module_registry::{
    contributions::{Access, Embed, EmbedContext, Method, Route, EMBEDS, ROUTES},
    Contribution, Provenance,
};
use bilibi_settings::{Definition, ScopeKind, Setting, Validator, DEFINITIONS};

/// Belimbing's durable account-administration keys (Bilimbi
/// `Core.User.Contributions`). K9 asks `view` to read an account's links and
/// `update` for the link commands; K13/K16 bind the rest to their screens.
pub const LIST: &str = "admin.user.list";
pub const VIEW: &str = "admin.user.view";
pub const CREATE: &str = "admin.user.create";
pub const UPDATE: &str = "admin.user.update";
pub const DELETE: &str = "admin.user.delete";
const READ: &[&str] = &[LIST, VIEW];
const ALL: &[&str] = &[LIST, VIEW, CREATE, UPDATE, DELETE];
const fn role(
    code: &'static str,
    name: &'static str,
    description: &'static str,
    capabilities: &'static [&'static str],
) -> RoleDefinition {
    RoleDefinition {
        code,
        name,
        description,
        grant_all: false,
        capabilities,
    }
}
// Bilimbi's two user-management roles, and the tenant owner's share.
inventory::submit! { Contribution::new(VOCABULARY, "core/user", Provenance { module: "core/user", source: file!(), line: line!() }, &VocabularyContribution {
    domains: &[], verbs: &[],
    capabilities: &[C::tenant(LIST), C::tenant(VIEW), C::tenant(CREATE), C::tenant(UPDATE), C::tenant(DELETE)],
    roles: &[
        role("user_viewer", "User Viewer", "Read-only access to user management.", READ),
        role("user_editor", "User Editor", "Read-write access to user management.", ALL),
        role("tenant_owner", "Tenant Owner", "Contributed system role; reconciled explicitly", ALL),
    ],
    system_principals: &[],
}) }

// The account administration pages (Bilimbi `FormLive`, `ShowLive`). The
// tenant-wide `/users` index is User Administration's (K13); these two are the
// create form and the read-first detail page it links to.
inventory::submit! {
    Contribution::new(ROUTES, "core/user.create", Provenance { module: "core/user", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/users/new", service: "core/user.pages", operation: "create", access: Access::Capability(CREATE), framed: true,
    })
}
inventory::submit! {
    Contribution::new(ROUTES, "core/user.detail", Provenance { module: "core/user", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/users/{id}", service: "core/user.pages", operation: "detail", access: Access::Capability(VIEW), framed: true,
    })
}

// The signed-in account's own name, email and landing page (Bilimbi
// `ProfileLive`); the session alone names the account.
inventory::submit! {
    Contribution::new(ROUTES, "core/user.settings.profile", Provenance { module: "core/user", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/settings/profile", service: "core/user.pages", operation: "profile", access: Access::SignedIn, framed: true,
    })
}
// Bilimbi `ui.landing_menu_id`: the menu page a sign-in opens, at the
// account's own scope; the profile page validates it against the menu the
// account can reach, since the definition can only bound its shape.
fn landing_accepts(value: &serde_json::Value) -> bool {
    value
        .as_str()
        .is_some_and(|id| id.len() <= 255 && !id.contains(char::is_control))
}
inventory::submit! {
    Contribution::new(DEFINITIONS, crate::accounts::LANDING_KEY, Provenance { module: "core/user", source: file!(), line: line!() }, &Setting::Definition(Definition {
        value_type: bilibi_settings::ValueType::String, scopes: &[ScopeKind::User], default: || serde_json::json!(""),
        nullable: false, encrypted: false, minimum: None, maximum: None,
        validator: Some(Validator { accepts: landing_accepts, message: "must name a menu page" }),
        editor: None,
    }))
}

inventory::submit! {
    Contribution::new(ROUTES, "core/user.settings.password", Provenance { module: "core/user", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/settings/password", service: "core/user.pages", operation: "password", access: Access::SignedIn, framed: true,
    })
}

inventory::submit! {
    Contribution::new(ROUTES, "core/user.settings.appearance", Provenance { module: "core/user", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/settings/appearance", service: "core/user.pages", operation: "appearance", access: Access::SignedIn, framed: true,
    })
}

// No capability: the page and its writes act only on the signed-in account
// (Bilimbi guards it by sign-in alone).
inventory::submit! {
    Contribution::new(ROUTES, "core/user.notifications.index", Provenance { module: "core/user", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/notifications", service: "core/user.pages", operation: "notifications", access: Access::SignedIn, framed: true,
    })
}

// The top-bar bell. Web renders and mounts every `EmbedContext::Shell`
// contribution itself, keyed by the slot's element id (`app-notifications`).
inventory::submit! {
    Contribution::new(EMBEDS, "core/user.notifications", Provenance { module: "core/user", source: file!(), line: line!() }, &Embed {
        slot: "shell.notifications", context: EmbedContext::Shell, service: "core/user.pages", operation: "bell", access: Access::SignedIn,
    })
}
