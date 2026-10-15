//! Shared UI: document/shell templates, the component primitives in
//! `templates/components.html` with their typed models, the feedback, dialog
//! and tab components (`feedback.rs`), the icon registry and
//! the vendored asset allowlist. Only shared fragments may use `safe`.
use askama::Template;
pub mod page;

// Lets the shared macro file name this crate's items as `bilibi_ui::...` from
// inside the crate and from every importing module alike.
extern crate self as bilibi_ui;

mod auth;
pub mod components;
mod feedback;
mod feedback_specimen;
pub mod forms;
mod html;
pub mod icons;
mod shell;
mod specimen;
pub use shell::{navigation, AppShell, DisplayPreferences, Framed, NavNode, ShellContext};
pub type Shell<'a> = AppShell<'a>;

pub use auth::{
    AuthLayout, ForgotForm, ForgotPasswordPage, LoginForm, LoginPage, ResetForm, ResetPasswordPage,
    CREDENTIALS_REJECTED, PASSWORD_RESET, RESET_LINK_INVALID, RESET_REQUESTED, SESSION_EXPIRED,
};
pub use components::{card_flat, card_pads, stat_strip_columns};
pub use components::{
    ActionLink, BackLink, Badge, BadgeKind, Button, ButtonVariant, Card, Chrome, CreateButton,
    Fact, Facts, Header, Icon, IconButton, IconContext, IconKind, Page, PageVariant,
    SectionHeading, StatItem, StatKind, StatStrip, DEMOTED_ACTION_CLASS, NO_COUNT,
};
pub use feedback::{
    Alert, ConfirmDialog, ConnectionBanners, Flash, FlashGroup, FlashMap, Modal, ModalWidth,
    NoticeKind, PanelNotice, Revealed, StatusKind, Tab, Tabs, AUTO_DISMISS_MS, NO_FLASH,
};
pub use feedback_specimen::FeedbackSpecimen;
pub use forms::{
    choices, described_by, field_base_class, field_class, field_state_class, translate_error,
    Attrs, Choice, CodeInput, Combobox, Field, InputKind, MultiSelect, RadioGroup, Restricted,
    RestrictedField, Reveal, SecretInput, StoredReveal, RESTRICTED_REASON, SECRET_MASK,
};
pub use html::Html;
pub use specimen::ComponentsSpecimen;

/// The HTML document around a page. `title` follows Bilimbi's live title:
/// the page's own name, or the product name when a page has none.
#[derive(Template)]
#[template(path = "document.html")]
pub struct Document<'a> {
    pub title: &'a str,
    pub body: &'a str,
}

pub const CSS: &str = include_str!("../assets/app.css");
pub const JS: &str = include_str!("../assets/live.js");
/// The hook controllers of the feedback, dialog and tab components.
pub const SHELL_JS: &str = include_str!("../assets/shell.js");
pub const UI_JS: &str = include_str!("../assets/ui.js");
/// The `data-hook` browser behaviour of the shared form controls.
pub const HOOKS_JS: &str = include_str!("../assets/hooks.js");
/// Bilimbi's vendored page-loading bar (MIT), shown by live.js for page-level round trips.
pub const TOPBAR_JS: &str = include_str!("../assets/vendor/topbar.js");

/// Vendored public assets. Keep this allowlist bounded: never resolve a
/// request path on disk, and never make font loading depend on authentication.
pub struct StaticAsset {
    pub content_type: &'static str,
    pub bytes: &'static [u8],
}

pub fn static_asset(name: &str) -> Option<StaticAsset> {
    let (content_type, bytes): (&str, &[u8]) = match name {
        "instrument-sans-variable.woff2" => (
            "font/woff2",
            include_bytes!("../assets/static/fonts/instrument-sans-variable.woff2"),
        ),
        "instrument-sans-variable-italic.woff2" => (
            "font/woff2",
            include_bytes!("../assets/static/fonts/instrument-sans-variable-italic.woff2"),
        ),
        "OFL.txt" => (
            "text/plain; charset=utf-8",
            include_bytes!("../assets/static/fonts/OFL.txt"),
        ),
        "ui-specimen.html" => (
            "text/html; charset=utf-8",
            include_bytes!("../assets/static/ui-specimen.html"),
        ),
        "ui-feedback-demo.js" => (
            "text/javascript",
            include_bytes!("../assets/static/ui-feedback-demo.js"),
        ),
        "favicon.svg" => (
            "image/svg+xml",
            include_bytes!("../assets/static/favicon.svg"),
        ),
        "favicon.ico" => (
            "image/x-icon",
            include_bytes!("../assets/static/favicon.ico"),
        ),
        _ => return None,
    };
    Some(StaticAsset {
        content_type,
        bytes,
    })
}

pub struct Renderer;
impl Renderer {
    pub fn render<T: Template>(&self, template: T) -> std::result::Result<String, askama::Error> {
        template.render()
    }
}
inventory::submit! {
    bilibi_module_registry::Module {
        id:"base/ui",layer:bilibi_module_registry::Layer::Base,dependencies:&["base/module_registry"],migrations:&[],
        services:&[bilibi_module_registry::Service {id:"base/ui.renderer",factory:|_|Ok(std::sync::Arc::new(Renderer))}],
    }
}
