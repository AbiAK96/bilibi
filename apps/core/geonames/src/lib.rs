//! Global geographic reference data: countries, first-level divisions,
//! postcodes and cities. A fresh install creates the tables and no rows; every
//! lookup answers empty until an operator imports (C2/C3). See `README.md`.
mod contributions;
mod download;
mod importer;
mod lookup;
mod model;
mod query;
mod reference_data;

pub use contributions::{LIST, UPDATE, VIEW};
pub use lookup::{normalize_iso, Geonames};
pub use model::*;
pub use query::*;

pub use download::{DownloadFailure, DownloadStatus, FallbackCause};
pub use importer::ImportFailure;
pub use reference_data::{
    default_cache_dir, Dataset, DatasetReport, ImportError, ImportOptions, ReferenceImport,
    DEFAULT_BASE_URL, IMPORT_SERVICE, USAGE,
};

use bilibi_database::Db;
use bilibi_integration::Integration;
use bilibi_module_registry::{Layer, Module, Service};
use std::sync::Arc;

/// The registry service id of the lookup API.
pub const SERVICE: &str = "core/geonames.lookup";
pub const MIGRATION: (&str, &str) = (
    "core-geonames-001",
    include_str!("../migrations/001-geonames.sql"),
);

inventory::submit! {
    Module {
        id: "core/geonames", layer: Layer::Core,
        dependencies: &["base/module_registry", "base/database", "base/authz", "base/grid", "base/integration", "base/tenancy"],
        migrations: &[MIGRATION],
        services: &[
            Service { id: SERVICE, factory: |services| {
                Ok(Arc::new(Geonames::new(services.get::<Db>("base/database.pool")?.as_ref().clone())))
            } },
            Service { id: IMPORT_SERVICE, factory: |services| {
                Ok(Arc::new(ReferenceImport::new(
                    services.get::<Db>("base/database.pool")?.as_ref().clone(),
                    services.get::<Integration>(bilibi_integration::SERVICE)?,
                )))
            } },
        ],
    }
}
