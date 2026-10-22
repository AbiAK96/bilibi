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

inventory::submit! {
    Module {
        id: "base/custom_fields", layer: Layer::Base,
        dependencies: &["base/module_registry", "base/database", "base/authz", "base/tenancy"],
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
