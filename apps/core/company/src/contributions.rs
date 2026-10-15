//! What Company adds to the shared catalogs (Bilimbi
//! `Bilimbi.Core.Company.Contributions`): its capability vocabulary and the
//! roles that carry it, the Company pages' routes and the Companies menu
//! entry. Web owns dispatch and scope guards; a route here bypasses nothing.
use bilibi_authz::{CapabilityDefinition as C, RoleDefinition, VocabularyContribution, VOCABULARY};
use bilibi_grid::{Field, FieldType::*, GridContribution, Source, TableDecl};
use bilibi_menu::{MenuItem, Requirement};
use bilibi_module_registry::{
    contributions::{Access, Method, Route, ROUTES},
    Contribution, Provenance,
};

/// Belimbing's durable keys, shared by the pages, catalog and business APIs.
pub const VIEW: &str = "admin.company.view";
pub const LIST: &str = "admin.company.list";
pub const CREATE: &str = "admin.company.create";
pub const UPDATE: &str = "admin.company.update";
pub const DELETE: &str = "admin.company.delete";
/// Reach beyond the actor's own company to any company of the tenant. It
/// never authorizes an operation by itself.
pub const TENANT_WIDE: &str = "admin.company.tenant-wide.manage";
pub const CAPABILITIES: [&str; 6] = [VIEW, LIST, CREATE, UPDATE, DELETE, TENANT_WIDE];

macro_rules! provenance {
    () => {
        Provenance {
            module: "core/company",
            source: file!(),
            line: line!(),
        }
    };
}
const fn role(
    code: &'static str,
    name: &'static str,
    capabilities: &'static [&'static str],
) -> RoleDefinition {
    RoleDefinition {
        code,
        name,
        description: "Contributed system role; reconciled explicitly",
        grant_all: false,
        capabilities,
    }
}
// `tenant_owner` receives the whole set as the source grants it. The demo
// roles are extended so the seeded accounts can try the K1 APIs: the manager
// administers any company of the tenant, staff edits, the viewer reads.
inventory::submit! { Contribution::new(VOCABULARY, "core/company", provenance!(), &VocabularyContribution {
    domains: &[], verbs: &[],
    capabilities: &[C::tenant(VIEW), C::tenant(LIST), C::tenant(CREATE), C::tenant(UPDATE), C::tenant(DELETE), C::tenant(TENANT_WIDE)],
    roles: &[
        role("tenant_owner", "Tenant Owner", &CAPABILITIES),
        role("manager", "manager", &CAPABILITIES),
        role("staff", "staff", &[VIEW, LIST, UPDATE]),
        role("viewer", "viewer", &[VIEW, LIST]),
    ],
    system_principals: &[],
}) }

inventory::submit! {
    Contribution::new(ROUTES, "core/company.list", provenance!(), &Route {
        method: Method::Get, path: "/companies", service: "core/company.pages", operation: "list", access: Access::Capability(LIST), framed: true,
    })
}
inventory::submit! {
    Contribution::new(ROUTES, "core/company.detail", provenance!(), &Route {
        method: Method::Get, path: "/companies/{id}", service: "core/company.pages", operation: "detail", access: Access::Capability(VIEW), framed: true,
    })
}
bilibi_menu::contribute!(
    "core/company",
    MenuItem::page("admin.company", "Companies", "/companies")
        .icon("building-office-2")
        .under("admin")
        .requires(Requirement::Capability(LIST))
        .order(10)
);

// The Companies catalog includes the Organisation facts. `record_types`
// names the audit kind a company is written under, so a restriction on a
// company field reaches the Audit screens and a record's History
// (`bilibi_authz::fields::withheld_by_record_type`).
static GRID: GridContribution = GridContribution {
    tables: &[TableDecl::new(
        "companies",
        "Companies",
        LIST,
        Source::tenant(
            "SELECT id, parent_id, code, name, legal_name, registration_number, legal_entity_type_id, jurisdiction, website, status, email, phone, tax_id, revenue, created_at FROM companies WHERE tenant_id = $1 AND deleted_at IS NULL",
        ),
        "id",
        &[
            Field::new("id", "ID", Integer),
            Field::new("code", "Code", String),
            Field::new("legal_name", "Legal Name", String),
            Field::new("registration_number", "Registration Number", String),
            Field::new("jurisdiction", "Jurisdiction", String),
            Field::new("website", "Website", String),
            Field::new("parent_id", "Parent", Integer).hidden(),
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

inventory::submit! {
    Contribution::new(ROUTES, "core/company.create", provenance!(), &Route {
        method: Method::Get, path: "/companies/create", service: "core/company.pages", operation: "create", access: Access::Capability(CREATE), framed: true,
    })
}
