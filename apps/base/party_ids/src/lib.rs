//! Shared wire identities only. An ID is never tenant membership or authorization
//! proof; owning Core providers must resolve it in a freshly authorized scope.
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidId;
impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("party IDs must be positive signed 64-bit integers")
    }
}
impl std::error::Error for InvalidId {}

macro_rules! identity {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize, Serialize,
        )]
        #[serde(try_from = "i64", into = "i64")]
        pub struct $name(i64);
        impl $name {
            pub const fn new(value: i64) -> Result<Self, InvalidId> {
                if value > 0 {
                    Ok(Self(value))
                } else {
                    Err(InvalidId)
                }
            }
            pub const fn get(self) -> i64 {
                self.0
            }
        }
        impl TryFrom<i64> for $name {
            type Error = InvalidId;
            fn try_from(value: i64) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
        impl From<$name> for i64 {
            fn from(value: $name) -> Self {
                value.get()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
identity!(PersonId);
identity!(OrganisationId);
identity!(AffiliationId);
identity!(RelationshipRoleId);
identity!(RelationshipId);

/// Business relationship target. The discriminant is retained in JSON and SQL;
/// equal numeric IDs in the two identity spaces must never be conflated.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PartyId {
    Organisation(OrganisationId),
    Person(PersonId),
}

inventory::submit! {
    bilibi_module_registry::Module {
        id: "base/party_ids", layer: bilibi_module_registry::Layer::Base,
        dependencies: &["base/module_registry"], migrations: &[], services: &[],
    }
}
