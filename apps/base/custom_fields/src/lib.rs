//! Tenant-defined record fields (Bilimbi `CustomFields`): definition ownership
//! and owner-side validation. Values remain in the opted-in record owner's row.
mod definitions;
mod model;
pub use definitions::{CustomFields, FieldError, FieldInput, Outcome, ServiceError};
pub use model::{Definition, FieldType, RecordType, RECORD_TYPES, SERVICE};

use bilibi_authz::{CapabilityDefinition, VocabularyContribution, VOCABULARY};
use bilibi_database::Db;
use bilibi_module_registry::Contribution;
use bilibi_module_registry::{Layer, Module, Service};
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
        dependencies: &["base/module_registry", "base/database", "base/authz", "base/tenancy", "base/grid", "base/ports"],
        migrations: &[("base-custom-fields-001-definitions", include_str!("../migrations/001-definitions.sql"))],
        services: &[Service { id: SERVICE, factory: |services| Ok(Arc::new(CustomFields::new(
            services.get::<Db>("base/database.pool")?.as_ref().clone(),
            services.get("base/database.mutations")?,
            services.get("base/tenancy.authentication")?,
        )?)) }],
    }
}

inventory::submit! { Contribution::new(VOCABULARY, "custom_fields.vocabulary", bilibi_module_registry::Provenance { module: "base/custom_fields", source: file!(), line: line!() }, &VocabularyContribution {
    domains: &["admin"], verbs: &["manage"], capabilities: &[CapabilityDefinition::tenant(definitions::MANAGE)], roles: &[], system_principals: &[],
}) }
