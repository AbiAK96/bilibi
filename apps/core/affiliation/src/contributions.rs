use bilibi_authz::{CapabilityDefinition as C, RoleDefinition, VocabularyContribution, VOCABULARY};
use bilibi_grid::{Field, FieldType, GridContribution, Source, TableDecl};
use bilibi_module_registry::{Contribution, Provenance};

pub const LIST: &str = "admin.affiliation.list";
pub const VIEW: &str = "admin.affiliation.view";
pub const CREATE: &str = "admin.affiliation.create";
pub const UPDATE: &str = "admin.affiliation.update";
pub const UNLINK: &str = "admin.affiliation.unlink";
pub const PROVE: &str = "admin.affiliation.prove";
const CAPS: &[&str] = &[LIST, VIEW, CREATE, UPDATE, UNLINK, PROVE];
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
inventory::submit! { Contribution::new(VOCABULARY, "core/affiliation", Provenance { module: "core/affiliation", source: file!(), line: line!() }, &VocabularyContribution {
    domains: &[], verbs: &["unlink", "prove"],
    capabilities: &[C::tenant(LIST), C::tenant(VIEW), C::tenant(CREATE), C::tenant(UPDATE), C::tenant(UNLINK), C::tenant(PROVE)],
    roles: &[role("tenant_owner", "Tenant Owner", CAPS), role("manager", "manager", CAPS), role("staff", "staff", &[LIST, VIEW, CREATE, UPDATE]), role("viewer", "viewer", &[LIST, VIEW])],
    system_principals: &[],
}) }

// Work contacts share the same masking path as API results and audit evidence.
static GRID: GridContribution = GridContribution {
    tables: &[TableDecl::new("person_affiliations", "Affiliations", LIST,
        Source::tenant("SELECT id,person_id,organisation_id,title,work_email,work_phone,effective_from,effective_to,unlinked_at,version,created_at,updated_at FROM person_affiliations WHERE tenant_id=$1"), "id", &[
            Field::new("id", "ID", FieldType::Integer).protected(),
            Field::new("person_id", "Person", FieldType::Integer).protected(),
            Field::new("organisation_id", "Organisation", FieldType::Integer).protected(),
            Field::new("title", "Title", FieldType::String).protected(),
            Field::new("work_email", "Work email", FieldType::String),
            Field::new("work_phone", "Work phone", FieldType::String),
            Field::new("effective_from", "From", FieldType::Date).protected(),
            Field::new("effective_to", "To", FieldType::Date).protected(),
            Field::new("unlinked_at", "Unlinked", FieldType::DateTime).protected(),
            Field::new("version", "Version", FieldType::Integer).protected(),
            Field::new("created_at", "Created", FieldType::DateTime).protected(),
            Field::new("updated_at", "Updated", FieldType::DateTime).protected(),
        ]).time_field("created_at").record_types(&["affiliation"])], links: &[],
};
bilibi_grid::contribute!("core/affiliation", "affiliation.grid", &GRID);
