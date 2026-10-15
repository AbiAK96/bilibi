//! Shared coercion of untrusted query/form strings (Bilimbi UI.Params).

/// Trim, parse the entire value, and accept only a positive integer.
pub fn positive_integer(value: Option<&str>) -> Option<u64> {
    value?.trim().parse::<u64>().ok().filter(|n| *n > 0)
}

/// Database identities use a signed 64-bit representation.
pub fn positive_id(value: Option<&str>) -> Option<i64> {
    positive_integer(value).and_then(|n| i64::try_from(n).ok())
}

/// Whitespace is significant here; only the empty string becomes absent.
pub fn blank_to_none(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

pub fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim)
}

/// The string fields of a posted form namespace (`filters[search]` → `search`)
/// from a live event payload. Other keys and non-string values are ignored, and
/// at most 64 fields of at most 1 KiB each are kept.
pub fn form_values(
    payload: &serde_json::Value,
    form: &str,
) -> std::collections::HashMap<String, String> {
    let Some(object) = payload.as_object() else {
        return Default::default();
    };
    object
        .iter()
        .filter_map(|(key, value)| {
            let key = key
                .strip_prefix(form)?
                .strip_prefix('[')?
                .strip_suffix(']')?;
            let value = value.as_str().filter(|value| value.len() <= 1024)?;
            Some((key.to_owned(), value.to_owned()))
        })
        .take(64)
        .collect()
}
