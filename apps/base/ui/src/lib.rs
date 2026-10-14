//! Shared compact ledger shell. Only server-rendered fragments may use `safe`.
use askama::Template;

#[derive(Template)]
#[template(path = "shell.html")]
pub struct Shell<'a> {
    pub user: &'a str,
    pub email: &'a str,
    pub tenant: &'a str,
    pub csrf: &'a str,
    pub unread: i64,
    pub body: &'a str,
}

#[derive(Template)]
#[template(path = "document.html")]
pub struct Document<'a> {
    pub body: &'a str,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct Login<'a> {
    pub error: &'a str,
}

pub const CSS: &str = include_str!("../assets/app.css");
pub const JS: &str = include_str!("../assets/live.js");

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
