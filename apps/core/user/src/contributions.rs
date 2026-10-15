//! The shell consumes a contributed bell; Base UI never imports Core User.
//! The account's self-service pages are discovered routes served by
//! `core/user.pages`; Web establishes the session before dispatching.
use bilibi_module_registry::{
    contributions::{Access, Embed, EmbedContext, Method, Route, EMBEDS, ROUTES},
    Contribution, Provenance,
};

inventory::submit! {
    Contribution::new(ROUTES, "core/user.settings.password", Provenance { module: "core/user", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/settings/password", service: "core/user.pages", operation: "password", access: Access::SignedIn, framed: true,
    })
}

inventory::submit! {
    Contribution::new(EMBEDS, "core/user.notifications", Provenance { module: "core/user", source: file!(), line: line!() }, &Embed {
        slot: "shell.notifications", context: EmbedContext::Shell, service: "core/user.accounts", operation: "unread", access: Access::Capability("notifications.view"),
    })
}
