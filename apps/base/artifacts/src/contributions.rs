//! Installation policy, editable only through operator-authorized Settings.
use bilibi_authz::{CapabilityDefinition, VocabularyContribution, VOCABULARY};
use bilibi_module_registry::{Contribution, Provenance};
use bilibi_settings::{Definition, Editor, ScopeKind, Setting, Validator, ValueType, DEFINITIONS};
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
        setting!($key,$kind,$default,$nullable,$min,$max,None,$label,$help);
    };
    ($key:literal,$kind:ident,$default:expr,$nullable:expr,$min:expr,$max:expr,$validator:expr,$label:literal,$help:literal) => {
        inventory::submit! { Contribution::new(DEFINITIONS,$key,provenance!(),&Setting::Definition(Definition {
            value_type:ValueType::$kind,scopes:&[ScopeKind::Global],default:||$default,nullable:$nullable,encrypted:false,minimum:$min,maximum:$max,validator:$validator,
            editor:Some(Editor {group:"Artifacts",label:$label,help:$help,capability:Some(MANAGE),reveal_duration_ms:5000}),
        })) }
    };
}
setting!("artifacts.storage_root",String,json!(""),false,None,None,"Private document directory","Existing absolute 0700 directory, outside public/static/assets. Old documents keep their original root.");
setting!("artifacts.retention_days",Integer,Value::Null,true,Some(1.0),Some(36500.0),"Document retention days","Required before publication; expiry is captured at reservation. Records-grade documents carry a retain-until instead.");
setting!("artifacts.max_bytes",Integer,json!(10_485_760),false,Some(1.0),Some(536_870_912.0),"Maximum document bytes","Maximum buffered upload/generated document size, up to 512 MiB. Owners validate content before storage.");
setting!(
    "artifacts.purge_batch_size",
    Integer,
    json!(100),
    false,
    Some(1.0),
    Some(1000.0),
    "Document purge batch size",
    "Maximum documents considered by each owner-scoped retention call."
);
setting!(
    "artifacts.purge_retry_minutes",
    Integer,
    json!(60),
    false,
    Some(1.0),
    Some(525_600.0),
    "Document purge retry interval (minutes)",
    "Minimum wait before retention retries a document whose purge failed or was refused."
);
setting!(
    "artifacts.purge_max_attempts",
    Integer,
    json!(5),
    false,
    Some(1.0),
    Some(1000.0),
    "Document purge attempts before hold",
    "Failed or refused purges after which a document is held for an operator to retry or resolve."
);
setting!("pdf.chromium.binary",String,json!("chromium"),false,None,None,Some(Validator { accepts: |value| value.as_str().is_some_and(|name| !name.is_empty() && name.len() <= 1024 && (!name.contains('/') || name.starts_with('/'))), message: "must be an executable name on PATH or an absolute path" }),"PDF Chromium executable","Sandbox-capable Chromium or Chrome for HTML printing. A bare name is searched on PATH. Never run it with its sandbox disabled.");
setting!("pdf.qpdf.binary",String,json!("qpdf"),false,None,None,Some(Validator { accepts: |value| value.as_str().is_some_and(|name| !name.is_empty() && name.len() <= 1024 && (!name.contains('/') || name.starts_with('/'))), message: "must be an executable name on PATH or an absolute path" }),"PDF qpdf executable","qpdf 10.6 or newer, used to protect PDFs with AES-256. A bare name is searched on PATH. Without it password-protected rendering is refused, never stored unprotected.");
setting!(
    "pdf.qpdf.timeout_seconds",
    Integer,
    json!(60),
    false,
    Some(1.0),
    Some(600.0),
    "PDF protection timeout (seconds)",
    "Maximum time one qpdf run may take; the tool is killed at the deadline."
);
setting!(
    "pdf.render_timeout_seconds",
    Integer,
    json!(30),
    false,
    Some(1.0),
    Some(600.0),
    "PDF render timeout (seconds)",
    "Maximum time one Chromium render may run; the browser is killed at the deadline."
);
setting!("pdf.render_origin",String,json!(""),false,None,None,Some(Validator { accepts: |value| value.as_str().is_some_and(valid_origin), message: "must be empty or an http(s) origin with a host name or IPv4 address, no path" }),"PDF render origin","Application origin the browser can reach, such as http://127.0.0.1:3000. Signed render URLs only work on the node that issued them. Empty disables signed-URL printing; inline printing needs no origin.");
setting!(
    "pdf.signed_url_ttl_seconds",
    Integer,
    json!(60),
    false,
    Some(1.0),
    Some(600.0),
    "PDF render token lifetime (seconds)",
    "How long a single-use signed render URL stays valid."
);
setting!(
    "pdf.max_render_tokens",
    Integer,
    json!(100),
    false,
    Some(1.0),
    Some(10000.0),
    "Maximum pending PDF render tokens",
    "Maximum signed render requests held in memory on each node."
);
setting!(
    "pdf.paper.format",
    String,
    json!("A4"),
    false,
    None,
    None,
    Some(Validator {
        accepts: |value| value.as_str().is_some_and(crate::engine::valid_format),
        message: "must be A4, A3, A5, Letter, Legal or Tabloid"
    }),
    "PDF paper format",
    "Paper size for HTML PDFs: A4, A3, A5, Letter, Legal or Tabloid."
);
setting!(
    "pdf.paper.print_background",
    Boolean,
    json!(true),
    false,
    None,
    None,
    "Print PDF backgrounds",
    "Include template background graphics in HTML PDFs."
);
/// Empty, or `http(s)://host[:port]` with no credentials, path, query or fragment.
fn valid_origin(value: &str) -> bool {
    let Some(rest) = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
    else {
        return value.is_empty();
    };
    let (host, port) = rest
        .split_once(':')
        .map_or((rest, None), |(h, p)| (h, Some(p)));
    !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))
        && port.is_none_or(|p| (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}
