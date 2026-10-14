//! Explicit existing demo ID mapping. Accounts are NOT automatically people.
use crate::{OrganisationId, PersonId};

/// Existing companies 1..=62 belong to tenant 1; company 100 belongs to tenant 2.
/// Retain the numeric ID and the tenant, including every existing URL and link.
pub fn organisation(tenant: i64, company_id: i64) -> Option<OrganisationId> {
    match (tenant, company_id) {
        (1, 1..=62) | (2, 100) => OrganisationId::new(company_id).ok(),
        _ => None,
    }
}

/// No Person/Affiliation/RelationshipRole rows exist in the original demo.
/// Account 6 (AI Agent) is still an account, not an inferred agent Person.
#[derive(Clone, Copy, Debug)]
pub struct AccountMapping {
    pub tenant_id: i64,
    pub account_id: i64,
    pub person_id: Option<PersonId>,
}
pub const ACCOUNTS: &[AccountMapping] = &[
    AccountMapping {
        tenant_id: 1,
        account_id: 1,
        person_id: None,
    },
    AccountMapping {
        tenant_id: 1,
        account_id: 2,
        person_id: None,
    },
    AccountMapping {
        tenant_id: 1,
        account_id: 3,
        person_id: None,
    },
    AccountMapping {
        tenant_id: 1,
        account_id: 4,
        person_id: None,
    },
    AccountMapping {
        tenant_id: 2,
        account_id: 5,
        person_id: None,
    },
    AccountMapping {
        tenant_id: 1,
        account_id: 6,
        person_id: None,
    },
];
