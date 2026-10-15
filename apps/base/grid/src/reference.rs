//! The reference tables: non-business stock data owned by Grid so every later
//! slice (query, lenses, page columns, Design Library specimen) has a catalog to
//! run against. Groups, items and movements exercise a one-link, many-links, a
//! chained one-link after a many-link, an enum, decimals, dates, a timestamp and a restrictable
//! field (`reference_items.unit_cost`). A manager, viewer or staff member may read
//! them after `authz-reconcile`.
use crate::model::{
    Field, FieldType::*, GridContribution, LinkDecl, LinkKind::*, Source, TableDecl,
};
use bilibi_authz::{CapabilityDefinition as C, RoleDefinition, VocabularyContribution, VOCABULARY};

const READ: &str = "grid.reference.view";

static GRID: GridContribution = GridContribution {
    tables: &[
        TableDecl::new(
            "reference_groups",
            "Reference groups",
            READ,
            Source::tenant("SELECT id, name, region FROM base_grid_ref_groups WHERE tenant_id = $1"),
            "id",
            &[
                Field::new("id", "ID", Integer),
                Field::new("name", "Name", String),
                Field::choice("region", "Region", &["north", "south", "east", "west"]),
            ],
        )
        .label_field("name"),
        TableDecl::new(
            "reference_items",
            "Reference items",
            READ,
            Source::tenant(
                "SELECT id, group_id, name, category, quantity, reorder_level, unit_cost, active FROM base_grid_ref_items WHERE tenant_id = $1",
            ),
            "id",
            &[
                Field::new("id", "ID", Integer),
                Field::new("group_id", "Group", Integer).hidden(),
                Field::new("name", "Name", String),
                Field::choice("category", "Category", &["tools", "parts", "supplies"]),
                Field::new("quantity", "Quantity", Integer),
                Field::new("reorder_level", "Reorder level", Integer),
                Field::new("unit_cost", "Unit cost", Decimal),
                Field::new("active", "Active", Boolean),
            ],
        )
        .label_field("name")
        .record_types(&["reference_item"]),
        TableDecl::new(
            "reference_movements",
            "Reference movements",
            READ,
            Source::tenant("SELECT id, item_id, delta, moved_on, recorded_at FROM base_grid_ref_movements WHERE tenant_id = $1"),
            "id",
            &[
                Field::new("id", "ID", Integer),
                Field::new("item_id", "Item", Integer).hidden(),
                Field::new("delta", "Change", Integer),
                Field::new("moved_on", "Moved on", Date),
                Field::new("recorded_at", "Recorded at", DateTime),
            ],
        )
        .time_field("moved_on"),
    ],
    links: &[
        LinkDecl::on("group", "Group", One, "reference_items", "group_id", "reference_groups", "id"),
        LinkDecl::on("items", "Items", Many, "reference_groups", "id", "reference_items", "group_id"),
        LinkDecl::on("movements", "Movements", Many, "reference_items", "id", "reference_movements", "item_id"),
        LinkDecl::on("item", "Item", One, "reference_movements", "item_id", "reference_items", "id"),
    ],
};
crate::contribute!("base/grid", "grid.reference", &GRID);

inventory::submit! { bilibi_module_registry::Contribution::new(VOCABULARY, "grid.reference.vocabulary",
bilibi_module_registry::Provenance { module: "base/grid", source: file!(), line: line!() },
&VocabularyContribution {
    domains: &["grid"],
    verbs: &["view"],
    capabilities: &[C::tenant(READ)],
    roles: &[
        RoleDefinition { code: "manager", name: "manager", description: "Contributed system role; reconciled explicitly", grant_all: false, capabilities: &[READ] },
        RoleDefinition { code: "viewer", name: "viewer", description: "Contributed system role; reconciled explicitly", grant_all: false, capabilities: &[READ] },
        RoleDefinition { code: "staff", name: "staff", description: "Contributed system role; reconciled explicitly", grant_all: false, capabilities: &[READ] },
    ],
    system_principals: &[],
}) }
