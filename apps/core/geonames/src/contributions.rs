//! What Geonames adds to shared catalogs: its capabilities and the Countries
//! grid table. Menu entries arrive with the screens they open (C4).
use bilibi_authz::{CapabilityDefinition, RoleDefinition, VocabularyContribution, VOCABULARY};
use bilibi_grid::{Field, FieldType::*, GridContribution, Source, TableDecl};
use bilibi_module_registry::{Contribution, Provenance};

/// Belimbing's durable keys. Reading is tenant-usable (the data is global and
/// harmless); editing a global table is an operator (platform) act.
pub const VIEW: &str = "admin.geonames.view";
pub const LIST: &str = "admin.geonames.list";
pub const UPDATE: &str = "admin.geonames.update";

inventory::submit! { Contribution::new(VOCABULARY, "core/geonames", Provenance { module: "core/geonames", source: file!(), line: line!() }, &VocabularyContribution {
    domains: &[],
    verbs: &[],
    capabilities: &[CapabilityDefinition::tenant(VIEW), CapabilityDefinition::tenant(LIST), CapabilityDefinition::platform(UPDATE)],
    roles: &[
        RoleDefinition { code: "tenant_owner", name: "Tenant Owner", description: "Contributed system role; reconciled explicitly", grant_all: false, capabilities: &[VIEW, LIST, UPDATE] },
    ],
    system_principals: &[],
}) }

// Global reference data: the source takes no tenant parameter. Rows are keyed
// by ISO code, which is how other tables point at a country.
static GRID: GridContribution = GridContribution {
    tables: &[TableDecl::new(
        "countries",
        "Countries",
        LIST,
        Source::global(
            "SELECT iso, iso3, country, capital, continent, population, area, currency_code, phone FROM geonames_countries",
        ),
        "iso",
        &[
            Field::new("iso", "ISO", String),
            Field::new("iso3", "ISO 3", String),
            Field::new("country", "Name", String),
            Field::new("capital", "Capital", String),
            Field::new("continent", "Continent", String),
            Field::new("population", "Population", Integer),
            Field::new("area", "Area", Float),
            Field::new("currency_code", "Currency", String),
            Field::new("phone", "Phone code", String),
        ],
    )
    .label_field("country")],
    links: &[],
};
bilibi_grid::contribute!("core/geonames", "geonames.grid", &GRID);
