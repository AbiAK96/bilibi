//! Restricted carries no original value. JSON conversion therefore cannot
//! accidentally reveal a masked payload, field name, or restriction reason.
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone)]
pub enum Data {
    Json(Value),
    Restricted,
    Object(BTreeMap<String, Data>),
    Array(Vec<Data>),
}
impl From<Value> for Data {
    fn from(value: Value) -> Self {
        Self::Json(value)
    }
}
impl Data {
    pub fn encode(self) -> Value {
        match self {
            Self::Json(value) => value,
            Self::Restricted => json!({"restricted":true}),
            Self::Object(fields) => Value::Object(
                fields
                    .into_iter()
                    .map(|(key, value)| (key, value.encode()))
                    .collect(),
            ),
            Self::Array(values) => Value::Array(values.into_iter().map(Self::encode).collect()),
        }
    }
    /// Owner-supplied restriction IDs are applied before serialization. Like
    /// Authz masking, this applies to a record or an array of records.
    pub fn restricted(value: Value, fields: &HashSet<String>) -> Self {
        match value {
            Value::Object(object) => Self::Object(
                object
                    .into_iter()
                    .map(|(key, value)| {
                        let value = if fields.contains(&key) {
                            Self::Restricted
                        } else {
                            Self::Json(value)
                        };
                        (key, value)
                    })
                    .collect(),
            ),
            Value::Array(rows) => Self::Array(
                rows.into_iter()
                    .map(|v| Self::restricted(v, fields))
                    .collect(),
            ),
            value => Self::Json(value),
        }
    }
}
pub struct Page {
    pub entries: Data,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
}
pub enum Output {
    Data(Data),
    Page(Page),
}
impl From<Value> for Output {
    fn from(value: Value) -> Self {
        Self::Data(value.into())
    }
}
impl Output {
    pub fn envelope(self, key: &str) -> Value {
        let mut meta = json!({"operation":key,"content_notice":crate::CONTENT_NOTICE});
        let result = match self {
            Self::Data(data) => data.encode(),
            Self::Page(page) => {
                meta["page"] =
                    json!({"page":page.page,"page_size":page.page_size,"total":page.total});
                page.entries.encode()
            }
        };
        json!({"result":result,"meta":meta})
    }
}
