use bilibi_authz::{CapabilityDefinition as C, RoleDefinition, VocabularyContribution, VOCABULARY};
use bilibi_grid::{Field, FieldType, GridContribution, LinkDecl, LinkKind, Source, TableDecl};
use bilibi_module_registry::{Contribution, Provenance};

// Bilimbi `Address.Contributions`. Attaching an address to a company or a
// Person is that owner's update, not one of these (`store::authorize_owner`).
pub const VIEW: &str = "admin.address.view";
pub const LIST: &str = "admin.address.list";
pub const CREATE: &str = "admin.address.create";
pub const UPDATE: &str = "admin.address.update";
pub const DELETE: &str = "admin.address.delete";
const CAPS: &[&str] = &[VIEW, LIST, CREATE, UPDATE, DELETE];
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
inventory::submit! { Contribution::new(VOCABULARY, "core/address", Provenance { module: "core/address", source: file!(), line: line!() }, &VocabularyContribution {
    domains: &[], verbs: &[],
    capabilities: &[C::tenant(VIEW), C::tenant(LIST), C::tenant(CREATE), C::tenant(UPDATE), C::tenant(DELETE)],
    roles: &[role("tenant_owner", "Tenant Owner", CAPS), role("manager", "manager", CAPS), role("staff", "staff", &[VIEW, LIST, CREATE, UPDATE]), role("viewer", "viewer", &[VIEW, LIST])],
    system_principals: &[],
}) }

// Bilimbi `Address.GridTables`: addresses with their country, and the links
// from owners to addresses. Address owns the attachments, so it declares the
// owner → address edges for both party kinds.
static GRID: GridContribution = GridContribution {
    tables: &[TableDecl::new("addresses", "Addresses", LIST,
        Source::tenant("SELECT id,label,line1,line2,locality,postcode,country_iso,phone,verification_status,created_at FROM addresses WHERE tenant_id=$1 AND deleted_at IS NULL"), "id", &[
            Field::new("id", "ID", FieldType::Integer).protected(),
            Field::new("label", "Label", FieldType::String).protected(),
            Field::new("line1", "Line 1", FieldType::String).protected(),
            Field::new("line2", "Line 2", FieldType::String).protected(),
            Field::new("locality", "Locality", FieldType::String).protected(),
            Field::new("postcode", "Postcode", FieldType::String).protected(),
            Field::new("country_iso", "Country code", FieldType::String).protected(),
            Field::new("phone", "Phone", FieldType::String),
            Field::choice("verification_status", "Verification", &["unverified", "suggested", "verified"]).protected(),
            Field::new("created_at", "Created", FieldType::DateTime).protected(),
        ]).label_field("label").time_field("created_at").record_types(&["address"])],
    links: &[
        LinkDecl::on("country", "Country", LinkKind::One, "addresses", "country_iso", "countries", "iso"),
        LinkDecl::edge("addresses", "Addresses", LinkKind::Many, "companies", "addresses",
            Source::tenant("SELECT t.organisation_id AS from_key, t.address_id AS to_key FROM address_attachments t JOIN addresses a ON a.tenant_id=t.tenant_id AND a.id=t.address_id WHERE t.tenant_id=$1 AND t.organisation_id IS NOT NULL AND a.deleted_at IS NULL")),
        // At most one per company: the attachment `primary` resolves to.
        LinkDecl::edge("primary_address", "Primary address", LinkKind::One, "companies", "addresses",
            Source::tenant("SELECT DISTINCT ON (t.organisation_id) t.organisation_id AS from_key, t.address_id AS to_key FROM address_attachments t JOIN addresses a ON a.tenant_id=t.tenant_id AND a.id=t.address_id WHERE t.tenant_id=$1 AND t.organisation_id IS NOT NULL AND a.deleted_at IS NULL ORDER BY t.organisation_id, t.is_primary DESC, t.priority, t.address_id")),
        LinkDecl::edge("addresses", "Addresses", LinkKind::Many, "persons", "addresses",
            Source::tenant("SELECT t.person_id AS from_key, t.address_id AS to_key FROM address_attachments t JOIN addresses a ON a.tenant_id=t.tenant_id AND a.id=t.address_id WHERE t.tenant_id=$1 AND t.person_id IS NOT NULL AND a.deleted_at IS NULL")),
    ],
};
bilibi_grid::contribute!("core/address", "address.grid", &GRID);
