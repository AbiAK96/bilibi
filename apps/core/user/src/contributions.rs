//! The shell consumes a contributed bell; Base UI never imports Core User.
//! `/notifications` is the bell's full list.
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
