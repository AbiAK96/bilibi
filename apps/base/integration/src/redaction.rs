//! Mandatory redaction at the persistence boundary, and again on every read.
//! Only JSON structure, numbers and booleans survive: a string can hold a bearer
//! token, a URL or any unstructured secret, so every string becomes `[omitted]`.
use serde_json::{json, Map, Value};

pub const REDACTED: &str = "[redacted]";
pub const OMITTED: &str = "[omitted]";
/// A retained preview larger than this, once redacted and encoded, records
/// only its omission.
pub const MAX_PREVIEW_BYTES: usize = 32_768;
const SECRET_KEYS: [&str; 8] = [
    "authorization",
    "cookie",
    "password",
    "secret",
    "token",
    "apikey",
    "credential",
    "signature",
];

/// `X-Api-Key`, `api_key` and `apiKey` all normalize to a name containing `apikey`.
fn secret_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect();
    SECRET_KEYS.iter().any(|secret| normalized.contains(secret))
}

pub fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| {
                    let item = if secret_key(key) {
                        Value::from(REDACTED)
                    } else {
                        redact(item)
                    };
                    (key.clone(), item)
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact).collect()),
        Value::String(_) => Value::from(OMITTED),
        Value::Number(_) | Value::Bool(_) | Value::Null => value.clone(),
    }
}

/// The stored form of an opted-in preview. `None` stays `None`.
pub fn structured(value: Option<&Value>) -> Option<Value> {
    match value? {
        value @ (Value::Object(_) | Value::Array(_)) => {
            let redacted = redact(value);
            let size = serde_json::to_vec(&redacted).map_or(usize::MAX, |bytes| bytes.len());
            Some(if size <= MAX_PREVIEW_BYTES {
                json!({"kind": "json", "value": redacted})
            } else {
                omitted("size_limit")
            })
        }
        _ => Some(omitted("unstructured")),
    }
}

/// Marker for a body that existed but was not kept (raw bytes, a stream, or
/// text that is not a JSON object or array).
pub fn omitted(reason: &str) -> Value {
    json!({"kind": "omitted", "reason": reason})
}

/// Payload reads redact again, so a row written by an older policy, or by hand,
/// can never become a second inspection surface.
pub fn inspect_stored(value: &Value) -> Value {
    if let Some(object) = value.as_object() {
        if object.len() == 2 {
            match (
                object.get("kind").and_then(Value::as_str),
                object.get("reason").and_then(Value::as_str),
                object.get("value"),
            ) {
                (Some("omitted"), Some(reason @ ("unstructured" | "size_limit")), _) => {
                    return omitted(reason)
                }
                (Some("json"), _, Some(inner)) => {
                    return json!({"kind": "json", "value": redact(inner)})
                }
                _ => {}
            }
        }
    }
    redact(value)
}

/// Removes URL credentials, the query string and the fragment. Text that is
/// not an absolute URL (a path or an operation key) loses everything from the
/// first `?` or `#`. Never put secrets in path segments.
pub fn endpoint(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut url) if url.has_host() => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        _ => raw.split(['?', '#']).next().unwrap_or_default().to_string(),
    }
}

/// `(host, path)` of an already sanitized endpoint; a non-URL is its own path.
pub fn location(endpoint: &str) -> (Option<String>, String) {
    match url::Url::parse(endpoint) {
        Ok(url) if url.has_host() => (url.host_str().map(str::to_string), url.path().to_string()),
        _ if endpoint.is_empty() => (None, "/".into()),
        _ => (None, endpoint.to_string()),
    }
}
