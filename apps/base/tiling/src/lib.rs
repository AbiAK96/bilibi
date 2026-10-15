//! The tiled workspace at `/workspace`: several pages side by side in one
//! browser tab, arranged by a Hyprland-style dwindle or master tree (Bilimbi
//! `Base.Tiling`).
//!
//! [`layout::Layout`] is the pure tree, every operation on it and the compact
//! URL form; the page in `web` holds only the tree, the focused tile and
//! monocle, and writes the tree back into `?t=` after every change, so a
//! reload or a copied address restores the screen. This module owns no tables.
pub mod layout;
mod web;

use bilibi_module_registry::contributions::{Access, Method, Route, ROUTES};
use std::sync::Arc;

inventory::submit! {
    bilibi_module_registry::Module {
        id: "base/tiling", layer: bilibi_module_registry::Layer::Base,
        dependencies: &["base/module_registry", "base/ui", "base/database", "base/tenancy", "base/authz", "base/menu"],
        migrations: &[], services: &[
            bilibi_module_registry::Service { id: "base/tiling.pages", factory: |services| {
                let catalog = services.catalog()?;
                let menu = bilibi_menu::Menu::from_catalog(catalog)?;
                let routes = catalog.entries::<Route>(ROUTES)?.iter()
                    .map(|entry| entry.get::<Route>())
                    .collect::<bilibi_module_registry::Result<Vec<_>>>()?;
                Ok(Arc::new(bilibi_ui::page::PageService::<bilibi_tenancy::Scope, bilibi_database::Error>(Arc::new(web::Pages::new(
                    services.get::<bilibi_database::Db>("base/database.pool")?.as_ref().clone(),
                    Arc::new(menu), routes,
                )))))
            }},
        ],
    }
}
// Any signed-in account may open the workspace: the page itself shows
// nothing, and each tile is judged by its own route's access exactly as the
// page is when opened alone. It has no menu entry; the sidebar tile control
// opens it. Framed, so a workspace opened inside a tile can say why it stops.
inventory::submit! { bilibi_module_registry::Contribution::new(ROUTES, "tiling.workspace",
bilibi_module_registry::Provenance { module: "base/tiling", source: file!(), line: line!() },
&Route { method: Method::Get, path: "/workspace", service: "base/tiling.pages", operation: "index", access: Access::SignedIn, framed: true }) }
