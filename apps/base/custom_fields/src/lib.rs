//! Tenant-defined record fields (Bilimbi `CustomFields`): definition ownership
//! and owner-side validation. Values remain in the opted-in record owner's row.
mod definitions;
mod model;
mod references;
mod web;
pub use definitions::{CustomFields, FieldError, FieldInput, Outcome, ServiceError};
pub use model::{Definition, FieldType, RecordType, RECORD_TYPES, SERVICE};
pub use references::{
    ReferenceFuture, ReferenceOption, ReferenceTargetProvider, ReferenceTargetSource,
    ReferenceTargetType, REFERENCE_TARGETS,
};

use bilibi_authz::{CapabilityDefinition, VocabularyContribution, VOCABULARY};
use bilibi_database::Db;
use bilibi_module_registry::Contribution;
use bilibi_module_registry::{
    contributions::{Access, Method, PageWrites, Pending, Route, PAGE_WRITES, ROUTES},
    Layer, Module, Service,
};
use std::sync::Arc;

struct Providers;

impl bilibi_grid::RuntimeFieldProvider for Providers {
    fn fields<'a>(
        &'a self,
        client: bilibi_ports::PortClient<'a>,
        scope: &'a bilibi_tenancy::Scope,
    ) -> bilibi_grid::RuntimeFuture<'a> {
        Box::pin(async move {
            bilibi_ports::on_client!(client, |sql| definitions::grid_fields(sql, scope).await)
        })
    }
}

impl bilibi_authz::field_catalog::RuntimeCatalogProvider for Providers {
    fn catalog<'a>(
        &'a self,
        client: bilibi_ports::PortClient<'a>,
        scope: &'a bilibi_tenancy::Scope,
    ) -> bilibi_authz::field_catalog::RuntimeFuture<'a> {
        Box::pin(async move {
            bilibi_ports::on_client!(client, |sql| definitions::authz_fields(sql, scope).await)
        })
    }
}

static PROVIDERS: Providers = Providers;

inventory::submit! { Contribution::new(bilibi_grid::RUNTIME_FIELDS, "custom_fields.grid_runtime", bilibi_module_registry::Provenance { module: "base/custom_fields", source: file!(), line: line!() }, &bilibi_grid::RuntimeFieldSource { provider: &PROVIDERS }) }
inventory::submit! { Contribution::new(bilibi_authz::field_catalog::RUNTIME_FIELD_CATALOG, "custom_fields.authz_runtime", bilibi_module_registry::Provenance { module: "base/custom_fields", source: file!(), line: line!() }, &bilibi_authz::field_catalog::RuntimeCatalogSource { provider: &PROVIDERS }) }

inventory::submit! {
    Module {
        id: "base/custom_fields", layer: Layer::Base,
        dependencies: &["base/module_registry", "base/database", "base/authz", "base/tenancy", "base/grid", "base/ports", "base/ui", "base/menu"],
        migrations: &[("base-custom-fields-001-definitions", include_str!("../migrations/001-definitions.sql"))],
        services: &[
            Service { id: SERVICE, factory: |services| Ok(Arc::new(CustomFields::new(
                services.get::<Db>("base/database.pool")?.as_ref().clone(),
                services.get("base/database.mutations")?,
                services.get("base/tenancy.authentication")?,
            )?)) },
            Service { id: web::PAGES, factory: |services| Ok(Arc::new(bilibi_ui::page::PageService::<bilibi_tenancy::Scope, bilibi_database::Error>(Arc::new(web::Pages {
                fields: services.get(SERVICE)?,
            })))) },
        ],
    }
}

inventory::submit! { Contribution::new(VOCABULARY, "custom_fields.vocabulary", bilibi_module_registry::Provenance { module: "base/custom_fields", source: file!(), line: line!() }, &VocabularyContribution {
    domains: &["admin"], verbs: &["manage"], capabilities: &[CapabilityDefinition::tenant(definitions::MANAGE)], roles: &[], system_principals: &[],
}) }

inventory::submit! { Contribution::new(ROUTES, "base/custom_fields.index", bilibi_module_registry::Provenance { module: "base/custom_fields", source: file!(), line: line!() }, &Route {
    method: Method::Get, path: web::PATH, service: web::PAGES, operation: "index",
    access: Access::Capability(definitions::MANAGE), framed: true,
}) }
bilibi_menu::contribute!(
    "base/custom_fields",
    bilibi_menu::MenuItem::page("admin.system.custom-fields", "Custom fields", web::PATH)
        .under("admin.system")
        .icon("settings")
        .requires(bilibi_menu::Requirement::Capability(definitions::MANAGE))
        .order(30)
);

// Definition writes remain on the tenant-scoped facade until the follow-up
// operation-contract slice. This mirrors Bilimbi `CustomFields` page writes.
inventory::submit! { Contribution::new(PAGE_WRITES, web::PAGES, bilibi_module_registry::Provenance { module: "base/custom_fields", source: file!(), line: line!() }, &PageWrites {
    service: web::PAGES,
    contracts: &[],
    pending: Pending::Events { events: &["save", "archive", "restore"], reason: "Custom-field definition writes use the audited Custom Fields facade; operation contracts are deferred to the agent-operations slice." },
}) }
