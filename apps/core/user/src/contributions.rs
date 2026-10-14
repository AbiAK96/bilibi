//! The shell consumes a contributed bell; Base UI never imports Core User.
use bilibi_module_registry::{
    contributions::{Access, Embed, EmbedContext, EMBEDS},
    Contribution, Provenance,
};

inventory::submit! {
    Contribution::new(EMBEDS, "core/user.notifications", Provenance { module: "core/user", source: file!(), line: line!() }, &Embed {
        slot: "shell.notifications", context: EmbedContext::Shell, service: "core/user.accounts", operation: "unread", access: Access::Capability("notifications.view"),
    })
}
