//! Grid catalog (G1): the tables, fields, links and sources every module declares,
//! the scoped catalog an account may read, and the bounded column-path parser.
//! There is no Grid page. Query, lenses and persisted arrangements build on this
//! catalog in later slices; see `README.md`.
pub mod catalog;
pub mod column;
pub mod model;
mod reference;

use bilibi_module_registry::{Consumer, Layer, Module, RegistryError, Result};
use std::sync::OnceLock;

pub use catalog::{
    restrictable, restriction_catalog, Catalog, CatalogError, Restrictable, MAX_COLUMNS,
};
pub use column::{Agg, Column, ColumnError, ColumnKind};
pub use model::{
    Field, FieldType, GridContribution, Installed, Join, Link, LinkDecl, LinkKind, Source, Table,
    TableDecl,
};

#[doc(hidden)]
pub use bilibi_module_registry::{Contribution, Provenance};
#[doc(hidden)]
pub use inventory;

/// The registry consumer every module's `GridContribution` is submitted to.
pub const TABLES: &str = "base/grid.tables";

/// Contribute tables and links from the owning module:
/// `bilibi_grid::contribute!("core/x", "x.grid", &GRID)` with
/// `static GRID: GridContribution`.
#[macro_export]
macro_rules! contribute {
    ($module:literal, $id:literal, $payload:expr) => {
        $crate::inventory::submit! {
            $crate::Contribution::new(
                $crate::TABLES,
                $id,
                $crate::Provenance { module: $module, source: file!(), line: line!() },
                $payload,
            )
        }
    };
}

/// The immutable deployment snapshot, validated once. Grants and restrictions are
/// never part of it.
pub fn installed() -> Result<&'static Installed> {
    static SNAPSHOT: OnceLock<Result<Installed>> = OnceLock::new();
    SNAPSHOT
        .get_or_init(|| {
            let installed = Installed::from_inventory()?;
            validate_capabilities(&installed)?;
            Ok(installed)
        })
        .as_ref()
        .map_err(|error| RegistryError::Contribution(error.to_string()))
}

/// A typo in a table's capability must fail at boot, not hide the table forever.
fn validate_capabilities(installed: &Installed) -> Result<()> {
    let vocabulary = bilibi_authz::vocabulary()
        .map_err(|_| RegistryError::Contribution("authz vocabulary unavailable".into()))?;
    for table in installed.tables() {
        if !vocabulary.capabilities.contains_key(table.capability()) {
            return Err(RegistryError::Contribution(format!(
                "grid table {} from {} ({}:{}) names undeclared capability {}",
                table.id(),
                table.owner,
                table.provenance.source,
                table.provenance.line,
                table.capability()
            )));
        }
    }
    Ok(())
}

inventory::submit! { Consumer::new::<GridContribution>(TABLES, "base/grid", |entries| Installed::from_entries(entries).map(|_| ())) }
inventory::submit! {
    Module {
        id: "base/grid", layer: Layer::Base,
        dependencies: &["base/module_registry", "base/database", "base/tenancy", "base/authz"],
        migrations: &[("base-grid-001", include_str!("../migrations/001-reference.sql"))],
        services: &[],
    }
}
inventory::submit! {
    bilibi_module_registry::Ready { module: "base/grid", check: |_| installed().map(|_| ()) }
}
