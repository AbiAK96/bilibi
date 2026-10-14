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
