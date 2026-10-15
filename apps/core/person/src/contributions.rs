use bilibi_authz::{CapabilityDefinition as C, RoleDefinition, VocabularyContribution, VOCABULARY};
use bilibi_grid::{Field, FieldType, GridContribution, Source, TableDecl};
use bilibi_module_registry::{Contribution, Provenance};

pub const LIST: &str = "admin.person.list";
pub const VIEW: &str = "admin.person.view";
pub const CREATE: &str = "admin.person.create";
pub const UPDATE: &str = "admin.person.update";
pub const RETIRE: &str = "admin.person.retire";
pub const SYSTEM_MANAGE: &str = "admin.person.system.manage";
const CAPS: &[&str] = &[LIST, VIEW, CREATE, UPDATE, RETIRE, SYSTEM_MANAGE];
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
inventory::submit! { Contribution::new(VOCABULARY, "core/person", Provenance { module: "core/person", source: file!(), line: line!() }, &VocabularyContribution {
    domains: &[], verbs: &["retire"],
    capabilities: &[C::tenant(LIST), C::tenant(VIEW), C::tenant(CREATE), C::tenant(UPDATE), C::tenant(RETIRE), C::platform(SYSTEM_MANAGE)],
    roles: &[role("tenant_owner", "Tenant Owner", CAPS), role("manager", "manager", CAPS), role("staff", "staff", &[LIST, VIEW, CREATE, UPDATE]), role("viewer", "viewer", &[LIST, VIEW])],
    system_principals: &[],
}) }

// Register contacts now so restrictions apply to every API and audit read.
// K8 composes the page from this catalog instead of defining a second table.
static GRID: GridContribution = GridContribution {
    tables: &[TableDecl::new("persons", "Persons", LIST,
        Source::tenant("SELECT id,name,kind,email,phone,system_key,retired_at,version,created_at,updated_at FROM persons WHERE tenant_id=$1"), "id", &[
            Field::new("id", "ID", FieldType::Integer).protected(),
            Field::new("name", "Name", FieldType::String).protected(),
            Field::choice("kind", "Kind", &["human", "agent"]).protected(),
            Field::new("email", "Personal email", FieldType::String),
            Field::new("phone", "Personal phone", FieldType::String),
            Field::new("system_key", "System identity", FieldType::String).protected(),
            Field::new("retired_at", "Retired", FieldType::DateTime).protected(),
            Field::new("version", "Version", FieldType::Integer).protected(),
            Field::new("created_at", "Created", FieldType::DateTime).protected(),
            Field::new("updated_at", "Updated", FieldType::DateTime).protected(),
        ]).label_field("name").time_field("created_at").record_types(&["person"])],
    links: &[],
};
bilibi_grid::contribute!("core/person", "person.grid", &GRID);
