//! Integration's contributed vocabulary, retention setting, scheduled payload
//! retention and the audit redaction for manually deleted exchanges.
use bilibi_audit::{Redaction, REDACTIONS};
use bilibi_authz::{CapabilityDefinition, RoleDefinition, VocabularyContribution, VOCABULARY};
use bilibi_module_registry::{Contribution, Provenance};
use bilibi_settings::{Definition, Editor, ScopeKind, Setting, ValueType, DEFINITIONS};
use serde_json::json;

/// Belimbing's durable keys cover both directions. Tenant-usable: an ordinary
/// tenant sees its own exchanges; only the operator scope widens visibility.
pub const LIST: &str = "admin.system.outbound-exchange.list";
pub const PAYLOAD: &str = "admin.system.outbound-exchange.payload.view";
pub const DELETE: &str = "admin.system.outbound-exchange.delete";
/// Global operator setting: days to keep redacted previews (metadata stays).
pub const RETENTION_DAYS: &str = "integration.payload_retention_days";
pub(crate) const RETENTION_DEFAULT: i64 = 30;
pub(crate) const RETENTION_MAX: i64 = 3650;

macro_rules! provenance {
    () => {
        Provenance {
            module: "base/integration",
            source: file!(),
            line: line!(),
        }
    };
}
inventory::submit! { Contribution::new(VOCABULARY, "base/integration", provenance!(), &VocabularyContribution {
    domains: &[],
    verbs: &[],
    capabilities: &[CapabilityDefinition::tenant(LIST), CapabilityDefinition::tenant(PAYLOAD), CapabilityDefinition::tenant(DELETE)],
    // Auditor and system viewer may list, never inspect payloads or delete.
    roles: &[
        RoleDefinition { code: "auditor", name: "Auditor", description: "Contributed system role; reconciled explicitly", grant_all: false, capabilities: &[LIST] },
        RoleDefinition { code: "system_viewer", name: "System Viewer", description: "Contributed system role; reconciled explicitly", grant_all: false, capabilities: &[LIST] },
    ],
    system_principals: &[],
}) }
inventory::submit! { Contribution::new(DEFINITIONS, RETENTION_DAYS, provenance!(), &Setting::Definition(Definition {
    value_type: ValueType::Integer, scopes: &[ScopeKind::Global], default: || json!(RETENTION_DEFAULT), nullable: false, encrypted: false,
    minimum: Some(1.0), maximum: Some(RETENTION_MAX as f64), validator: None,
    editor: Some(Editor { group: "Integration", label: "API exchange payload retention",
        help: "Days to retain redacted payload previews. Exchange metadata remains available.", capability: Some(DELETE), reveal_duration_ms: 5_000 }),
})) }
// Deleting an exchange is audited with metadata only; these names never carry
// a second copy of a payload or a raw endpoint into the audit ledger.
inventory::submit! { Contribution::new(REDACTIONS, "base/integration.exchange", provenance!(), &Redaction {
    kind: "base/integration.exchange",
    fields: &["endpoint", "request_headers", "request_body", "response_headers", "response_body", "error_message"],
}) }
