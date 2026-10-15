//! Operator settings (Administration → Settings → Operator, P3), the platform
//! capability that edits them, and audit redaction for the stored digests.
use bilibi_audit::{Redaction, REDACTIONS};
use bilibi_authz::{CapabilityDefinition, VocabularyContribution, VOCABULARY};
use bilibi_module_registry::{Contribution, Provenance};
use bilibi_settings::{Definition, Editor, ScopeKind, Setting, Validator, ValueType, DEFINITIONS};
use serde_json::json;

/// Operator settings are platform-only: a grant alone is not enough, the
/// persistent operator identity is required too.
pub const MANAGE: &str = "admin.system.agent-access.manage";
macro_rules! provenance {
    () => {
        Provenance {
            module: "core/agent_access",
            source: file!(),
            line: line!(),
        }
    };
}
const GLOBAL: &[ScopeKind] = &[ScopeKind::Global];

inventory::submit! { Contribution::new(VOCABULARY, "core/agent_access", provenance!(), &VocabularyContribution {
    capabilities: &[CapabilityDefinition::platform(MANAGE)], domains: &[], verbs: &[], roles: &[], system_principals: &[],
}) }

inventory::submit! { Contribution::new(DEFINITIONS, crate::CONNECTION_DAYS, provenance!(), &Setting::Definition(Definition {
    value_type: ValueType::Integer, scopes: GLOBAL, default: || json!(90), nullable: false, encrypted: false, minimum: Some(1.0), maximum: Some(365.0), validator: None,
    editor: Some(Editor { group: "Agent access", label: "Agent connection length (days)", help: "Length of new device and OAuth connections. Existing connections keep their end dates.", capability: Some(MANAGE), reveal_duration_ms: 5_000 }),
})) }
inventory::submit! { Contribution::new(DEFINITIONS, crate::OAUTH_CLIENTS, provenance!(), &Setting::Definition(Definition {
    value_type: ValueType::Array, scopes: GLOBAL, default: || json!({}), nullable: false, encrypted: false, minimum: None, maximum: None,
    validator: Some(Validator { accepts: crate::valid_clients, message: "Use client IDs mapped to a name and non-empty HTTPS redirect_uris list." }),
    editor: Some(Editor { group: "Agent access", label: "OAuth public clients", help: "Client IDs mapped to name and exact HTTPS redirect_uris. Register ChatGPT's callback here before connecting MCP.", capability: Some(MANAGE), reveal_duration_ms: 5_000 }),
})) }
inventory::submit! { Contribution::new(DEFINITIONS, crate::AUTHORIZATION_CODE_SECONDS, provenance!(), &Setting::Definition(Definition {
    value_type: ValueType::Integer, scopes: GLOBAL, default: || json!(120), nullable: false, encrypted: false, minimum: Some(30.0), maximum: Some(600.0), validator: None,
    editor: Some(Editor { group: "Agent access", label: "OAuth authorization code lifetime", help: "Seconds for a public client to redeem a consented authorization code with PKCE.", capability: Some(MANAGE), reveal_duration_ms: 5_000 }),
})) }
inventory::submit! { Contribution::new(DEFINITIONS, crate::OAUTH_REQUESTS_PER_MINUTE, provenance!(), &Setting::Definition(Definition {
    value_type: ValueType::Integer, scopes: GLOBAL, default: || json!(60), nullable: false, encrypted: false, minimum: Some(1.0), maximum: Some(10_000.0), validator: None,
    editor: Some(Editor { group: "Agent access", label: "OAuth requests per minute", help: "Maximum device or token requests per sender address per minute on each host node.", capability: Some(MANAGE), reveal_duration_ms: 5_000 }),
})) }

// Audit never receives a digest from this module, and the policy also masks the
// field names should a later change capture a whole row.
inventory::submit! { Contribution::new(REDACTIONS, "core/agent_access.devices", provenance!(), &Redaction { kind: "agent_device", fields: &["device_hash", "user_hash"] }) }
inventory::submit! { Contribution::new(REDACTIONS, "core/agent_access.tokens", provenance!(), &Redaction { kind: "agent_token", fields: &["token_hash"] }) }
inventory::submit! { Contribution::new(REDACTIONS, "core/agent_access.codes", provenance!(), &Redaction { kind: "agent_authorization_code", fields: &["code_hash", "code_challenge"] }) }
