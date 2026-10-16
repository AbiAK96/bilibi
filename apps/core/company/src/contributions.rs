//! What Company adds to the shared catalogs (Bilimbi
//! `Bilimbi.Core.Company.Contributions`): its capability vocabulary and the
//! roles that carry it, the Company pages' routes and the Companies menu
//! entry. Web owns dispatch and scope guards; a route here bypasses nothing.
use bilibi_authz::{CapabilityDefinition as C, RoleDefinition, VocabularyContribution, VOCABULARY};
use bilibi_grid::{Field, FieldType::*, GridContribution, LinkDecl, LinkKind, Source, TableDecl};
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
// The two global catalogs sit under Companies, as the source's menu nests
// them; their routes are operator pages (Bilimbi `operator: true`), which the
// shell's menu reflects through the route's access.
bilibi_menu::contribute!(
    "core/company",
    MenuItem::page(
        "admin.company.department-type",
        "Department Types",
        "/companies/department-types"
    )
    .under("admin.company")
    .requires(Requirement::Capability(LIST))
    .order(10)
);
bilibi_menu::contribute!(
    "core/company",
    MenuItem::page(
        "admin.company.legal-entity-type",
        "Legal Entity Types",
        "/companies/legal-entity-types"
    )
    .under("admin.company")
    .requires(Requirement::Capability(LIST))
    .order(20)
);
inventory::submit! {
    Contribution::new(ROUTES, "core/company.legal_entity_types", provenance!(), &Route {
        method: Method::Get, path: "/companies/legal-entity-types", service: "core/company.pages", operation: "legal_entity_types", access: Access::OperatorCapability(LIST), framed: true,
    })
}
inventory::submit! {
    Contribution::new(ROUTES, "core/company.department_types", provenance!(), &Route {
        method: Method::Get, path: "/companies/department-types", service: "core/company.pages", operation: "department_types", access: Access::OperatorCapability(LIST), framed: true,
    })
}
inventory::submit! {
    Contribution::new(ROUTES, "core/company.departments", provenance!(), &Route {
        method: Method::Get, path: "/companies/{id}/departments", service: "core/company.pages", operation: "departments", access: Access::Capability(VIEW), framed: true,
    })
}

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
            "SELECT id, parent_id, code, name, legal_name, registration_number, legal_entity_type_id, jurisdiction, website, status, email, phone, tax_id, revenue, created_at, updated_at FROM companies WHERE tenant_id = $1 AND deleted_at IS NULL",
        ),
        "id",
        &[
            Field::new("id", "ID", Integer),
            Field::new("code", "Code", String).protected(),
            Field::new("legal_name", "Legal Name", String).protected(),
            Field::new("registration_number", "Registration Number", String),
            Field::new("jurisdiction", "Jurisdiction", String),
            Field::new("website", "Website", String),
            Field::new("parent_id", "Parent", Integer).hidden(),
            Field::new("legal_entity_type_id", "Legal entity type", Integer).hidden(),
            Field::new("name", "Name", String),
            Field::choice("status", "Status", &["active", "suspended", "pending", "archived"]).protected(),
            Field::new("email", "Email", String),
            Field::new("phone", "Phone", String),
            Field::new("tax_id", "Tax ID", String),
            Field::new("revenue", "Revenue", Integer),
            Field::new("created_at", "Created", DateTime),
            Field::new("updated_at", "Updated", DateTime),
        ],
    )
    .label_field("name")
    .time_field("created_at")
    .record_types(&["company"]),
    // A department is in the tenant through its company, so the source joins
    // the tenant's live companies (Bilimbi `Grid.DepartmentsSource`); the
    // head is the affiliation id, which the affiliation owner links.
    TableDecl::new(
        "departments",
        "Departments",
        LIST,
        Source::tenant(
            "SELECT d.id, d.company_id, d.head_affiliation_id AS head_id, d.status, t.code AS type_code, t.name AS type_name, d.created_at FROM company_departments d JOIN companies c ON c.tenant_id = d.tenant_id AND c.id = d.company_id AND c.deleted_at IS NULL LEFT JOIN company_department_types t ON t.id = d.department_type_id WHERE d.tenant_id = $1",
        ),
        "id",
        &[
            Field::new("id", "ID", Integer),
            Field::new("type_name", "Type", String),
            Field::new("type_code", "Type code", String),
            Field::choice("status", "Status", &["active", "inactive", "suspended"]),
            Field::new("created_at", "Created", DateTime),
            Field::new("company_id", "Company", Integer).hidden(),
            Field::new("head_id", "Head", Integer).hidden(),
        ],
    )
    .label_field("type_name")
    .time_field("created_at"),
    // Global reference data: no tenant parameter (Bilimbi `Grid.LegalEntityTypesSource`).
    TableDecl::new(
        "legal_entity_types",
        "Legal entity types",
        LIST,
        Source::global("SELECT id, code, name, is_active FROM company_legal_entity_types"),
        "id",
        &[
            Field::new("id", "ID", Integer),
            Field::new("code", "Code", String),
            Field::new("name", "Name", String),
            Field::new("is_active", "Active", Boolean),
        ],
    )
    .label_field("name")],
    links: &[
        LinkDecl::on("parent", "Parent company", LinkKind::One, "companies", "parent_id", "companies", "id"),
        LinkDecl::on("children", "Subsidiaries", LinkKind::Many, "companies", "id", "companies", "parent_id"),
        LinkDecl::on("legal_entity_type", "Legal entity type", LinkKind::One, "companies", "legal_entity_type_id", "legal_entity_types", "id"),
        LinkDecl::on("departments", "Departments", LinkKind::Many, "companies", "id", "departments", "company_id"),
        LinkDecl::on("company", "Company", LinkKind::One, "departments", "company_id", "companies", "id"),
    ],
};
bilibi_grid::contribute!("core/company", "company.grid", &GRID);

inventory::submit! {
    Contribution::new(ROUTES, "core/company.create", provenance!(), &Route {
        method: Method::Get, path: "/companies/create", service: "core/company.pages", operation: "create", access: Access::Capability(CREATE), framed: true,
    })
}
