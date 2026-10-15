//! Reuse the registry's route contract; Web owns dispatch and scope guards.
use bilibi_grid::{Field, FieldType::*, GridContribution, Source, TableDecl};
use bilibi_menu::{MenuItem, Requirement};
use bilibi_module_registry::{
    contributions::{Access, Method, Route, ROUTES},
    Contribution, Provenance,
};

inventory::submit! {
    Contribution::new(ROUTES, "core/company.list", Provenance { module: "core/company", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/companies", service: "core/company.pages", operation: "list", access: Access::Capability("companies.view"), framed: true,
    })
}
inventory::submit! {
    Contribution::new(ROUTES, "core/company.detail", Provenance { module: "core/company", source: file!(), line: line!() }, &Route {
        method: Method::Get, path: "/companies/{id}", service: "core/company.pages", operation: "detail", access: Access::Capability("companies.view"), framed: true,
    })
}
bilibi_menu::contribute!(
    "core/company",
    MenuItem::page("admin.company", "Companies", "/companies")
        .icon("building-office-2")
        .under("admin")
        .requires(Requirement::Capability("companies.view"))
        .order(10)
);

// The Companies catalog table. Its restrictable fields are the ones the demo
// seed restricts (`email`, `phone`, `tax_id`, `revenue`), and `record_types`
// names the audit kind a company is written under, so a restriction on a
// company field reaches the Audit screens and a record's History
// (`bilibi_authz::fields::withheld_by_record_type`): the trail withholds what
// the company page withholds. The party-model Company replaces this table.
static GRID: GridContribution = GridContribution {
    tables: &[TableDecl::new(
        "companies",
        "Companies",
        "companies.view",
        Source::tenant(
            "SELECT id, name, status, email, phone, tax_id, revenue, created_at FROM companies WHERE tenant_id = $1 AND deleted_at IS NULL",
        ),
        "id",
        &[
            Field::new("id", "ID", Integer),
            Field::new("name", "Name", String),
            Field::choice("status", "Status", &["active", "suspended", "pending", "archived"]),
            Field::new("email", "Email", String),
            Field::new("phone", "Phone", String),
            Field::new("tax_id", "Tax ID", String),
            Field::new("revenue", "Revenue", Integer),
            Field::new("created_at", "Created", DateTime),
        ],
    )
    .label_field("name")
    .time_field("created_at")
    .record_types(&["company"])],
    links: &[],
};
bilibi_grid::contribute!("core/company", "company.grid", &GRID);
