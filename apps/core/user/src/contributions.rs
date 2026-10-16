//! The shell consumes a contributed bell; Base UI never imports Core User.
//! `/notifications` is the bell's full list.
//! The account's self-service pages are discovered routes served by
//! `core/user.pages`; Web establishes the session before dispatching.
use bilibi_authz::{CapabilityDefinition as C, RoleDefinition, VocabularyContribution, VOCABULARY};
use bilibi_module_registry::{
    contributions::{Access, Embed, EmbedContext, Method, Route, EMBEDS, ROUTES},
    Contribution, Provenance,
};

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
// Bilimbi's two user-management roles, plus the demo roles extended as the
// other Core owners extend them so the seeded accounts can try the commands.
inventory::submit! { Contribution::new(VOCABULARY, "core/user", Provenance { module: "core/user", source: file!(), line: line!() }, &VocabularyContribution {
    domains: &[], verbs: &[],
    capabilities: &[C::tenant(LIST), C::tenant(VIEW), C::tenant(CREATE), C::tenant(UPDATE), C::tenant(DELETE)],
    roles: &[
        role("user_viewer", "User Viewer", "Read-only access to user management.", READ),
        role("user_editor", "User Editor", "Read-write access to user management.", ALL),
        role("tenant_owner", "Tenant Owner", "Contributed system role; reconciled explicitly", ALL),
        role("manager", "manager", "Contributed system role; reconciled explicitly", ALL),
        role("staff", "staff", "Contributed system role; reconciled explicitly", READ),
        role("viewer", "viewer", "Contributed system role; reconciled explicitly", READ),
    ],
    system_principals: &[],
}) }

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

// No capability beyond reading one's own notifications: the page and its
// writes act only on the signed-in account (Bilimbi guards it by sign-in).
inventory::submit! {
    Contribution::new(ROUTES, "core/user.notifications.index", Provenance { module: "core/user", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/notifications", service: "core/user.pages", operation: "notifications", access: Access::Capability("notifications.view"), framed: true,
    })
}

// The top-bar bell. Web renders and mounts every `EmbedContext::Shell`
// contribution itself, keyed by the slot's element id (`app-notifications`).
inventory::submit! {
    Contribution::new(EMBEDS, "core/user.notifications", Provenance { module: "core/user", source: file!(), line: line!() }, &Embed {
        slot: "shell.notifications", context: EmbedContext::Shell, service: "core/user.pages", operation: "bell", access: Access::Capability("notifications.view"),
    })
}
