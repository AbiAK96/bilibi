//! Installation policy, editable only through operator-authorized Settings.
use bilibi_authz::{CapabilityDefinition, VocabularyContribution, VOCABULARY};
use bilibi_module_registry::{Contribution, Provenance};
use bilibi_settings::{Definition, Editor, ScopeKind, Setting, ValueType, DEFINITIONS};
use serde_json::{json, Value};
const MANAGE: &str = "admin.system.artifacts.manage";
macro_rules! provenance {
    () => {
        Provenance {
            module: "base/artifacts",
            source: file!(),
            line: line!(),
        }
    };
}
inventory::submit! { Contribution::new(VOCABULARY,"base/artifacts",provenance!(),&VocabularyContribution {
    capabilities:&[CapabilityDefinition::platform(MANAGE)], domains:&[],verbs:&[],roles:&[],system_principals:&[],
}) }
macro_rules! setting {
    ($key:literal,$kind:ident,$default:expr,$nullable:expr,$min:expr,$max:expr,$label:literal,$help:literal) => {
        inventory::submit! { Contribution::new(DEFINITIONS,$key,provenance!(),&Setting::Definition(Definition {
            value_type:ValueType::$kind,scopes:&[ScopeKind::Global],default:||$default,nullable:$nullable,encrypted:false,minimum:$min,maximum:$max,validator:None,
            editor:Some(Editor {group:"Artifacts",label:$label,help:$help,capability:Some(MANAGE),reveal_duration_ms:5000}),
        })) }
    };
}
setting!("artifacts.storage_root",String,json!(""),false,None,None,"Private document directory","Existing absolute 0700 directory, outside public/static/assets. Old documents keep their original root.");
setting!("artifacts.retention_days",Integer,Value::Null,true,Some(1.0),Some(36500.0),"Document retention days","Required before publication; expiry is captured at reservation. Records-grade retention follows in A2.");
setting!("artifacts.max_bytes",Integer,json!(10_485_760),false,Some(1.0),Some(536_870_912.0),"Maximum document bytes","Maximum buffered upload/generated document size, up to 512 MiB. Owners validate content before storage.");
