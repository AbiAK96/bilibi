//! Human action intent digest. A request row keeps only this digest of what
//! was asked, so a repeated request is recognized by recomputing it: SHA-256
//! over the canonical JSON of the subject, actor, action, work binding and
//! payload, with object keys sorted by byte, list order preserved and the
//! process fingerprint's string escaping. Floats are refused before any write
//! because no serializer choice may change a saved digest; send fractional
//! values as strings.
use crate::process::{canonical, exact};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) struct Intent<'a> {
    pub subject_type: &'a str,
    pub subject_id: i64,
    pub actor_type: &'a str,
    pub actor_id: i64,
    pub action_key: &'a str,
    pub process_run_id: Option<i64>,
    pub work_item_id: Option<i64>,
    pub payload: &'a Value,
}
/// `true` when `payload` has a stable digest: no floating-point numbers.
pub(crate) fn reproducible(payload: &Value) -> bool {
    exact(payload)
}
pub(crate) fn digest(intent: &Intent<'_>) -> String {
    let document = json!({
        "subject_type": intent.subject_type,
        "subject_id": intent.subject_id,
        "actor_type": intent.actor_type,
        "actor_id": intent.actor_id,
        "action_key": intent.action_key,
        "process_run_id": intent.process_run_id,
        "work_item_id": intent.work_item_id,
        "payload": intent.payload,
    });
    let mut out = String::new();
    canonical(&document, &mut out);
    Sha256::digest(out.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
/// Digests are not secret, but compare without an early exit anyway.
pub(crate) fn same(saved: &str, given: &str) -> bool {
    saved.len() == given.len()
        && saved
            .bytes()
            .zip(given.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}
