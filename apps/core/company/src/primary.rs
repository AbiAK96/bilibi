//! The explicit tenant → primary company relationship and the provisioning
//! commands (Bilimbi `PrimaryCompanyManager`, `mix bilimbi.tenant.provision`
//! and `mix bilimbi.platform.provision`). Company meaning is never inferred
//! from a numeric id or the oldest company of a tenant: a tenant stands on
//! the company `tenant_primary_companies` names, and nothing else.
use crate::{
    organisation::{
        check_references, display_name, insert, lock_code, validate, Identity, NewCompany,
    },
    Company, FieldError, WriteError,
};
use bilibi_audit::{record_action, Attribution};
use bilibi_database::{Change, Db, Error, MutationTransaction, Result};
use bilibi_party_ids::OrganisationId;
use bilibi_tenancy::{admin::TenantIdentity, Scope};
use serde_json::json;
use std::collections::HashSet;
use tokio_postgres::{GenericClient, Transaction};

/// The tenant's primary company, or `None` while the tenant has none. An
/// assignment that names a soft-deleted or missing company is an invariant
/// violation (`Error::Internal`), never silently `None`.
pub async fn primary_company(client: &impl GenericClient, tenant: i64) -> Result<Option<Identity>> {
    let Some(row) = client
        .query_opt(
            "SELECT a.company_id, c.code, c.legal_name, c.name, c.deleted_at IS NULL FROM tenant_primary_companies a LEFT JOIN companies c ON c.tenant_id=a.tenant_id AND c.id=a.company_id WHERE a.tenant_id=$1",
            &[&tenant],
        )
        .await?
    else {
        return Ok(None);
    };
    let code: Option<String> = row.get(1);
    let live: bool = row.get(4);
    let (Some(code), true) = (code, live) else {
        return Err(Error::Internal);
    };
    let legal_name: Option<String> = row.get(2);
    let name: String = row.get(3);
    Ok(Some(Identity {
        id: OrganisationId::new(row.get(0)).map_err(|_| Error::Internal)?,
        code,
        display_name: display_name(legal_name.as_deref(), &name),
    }))
}

/// Whether `company_id` is the primary company of the scope's tenant.
pub async fn is_primary(
    client: &impl GenericClient,
    scope: &Scope,
    company_id: i64,
) -> Result<bool> {
    Ok(client
        .query_opt(
            "SELECT 1 FROM tenant_primary_companies WHERE tenant_id=$1 AND company_id=$2",
            &[&scope.tenant(), &company_id],
        )
        .await?
        .is_some())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentStatus {
    Assigned,
    Unchanged,
    Transferred,
}
#[derive(Debug, thiserror::Error)]
pub enum AssignmentError {
    #[error("the tenant does not exist")]
    TenantNotFound,
    #[error("the company does not exist in this tenant")]
    CompanyNotFound,
    #[error("an archived company cannot be the primary company")]
    CompanyArchived,
    #[error("the tenant already stands on company {0}")]
    AlreadyAssigned(i64),
    #[error(transparent)]
    Access(#[from] Error),
}

/// Assigns (or, with `transfer`, moves) the tenant's primary company. Locks
/// the tenant row, then the company row, judges the company under that lock
/// and writes one `company.primary` change; `Unchanged` writes nothing.
pub async fn assign(
    tx: &mut MutationTransaction<'_>,
    tenant: i64,
    company_id: i64,
    transfer: bool,
) -> std::result::Result<AssignmentStatus, AssignmentError> {
    let sql = tx.sql();
    sql.query_opt(
        "SELECT id FROM tenants WHERE id=$1 AND deleted_at IS NULL FOR NO KEY UPDATE",
        &[&tenant],
    )
    .await
    .map_err(Error::from)?
    .ok_or(AssignmentError::TenantNotFound)?;
    let company = sql
        .query_opt(
            "SELECT status FROM companies WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE",
            &[&tenant, &company_id.max(0)],
        )
        .await
        .map_err(Error::from)?
        .ok_or(AssignmentError::CompanyNotFound)?;
    if company.get::<_, String>(0) == "archived" {
        return Err(AssignmentError::CompanyArchived);
    }
    let current: Option<i64> = sql
        .query_opt(
            "SELECT company_id FROM tenant_primary_companies WHERE tenant_id=$1 FOR UPDATE",
            &[&tenant],
        )
        .await
        .map_err(Error::from)?
        .map(|row| row.get(0));
    let status = match current {
        Some(current) if current == company_id => return Ok(AssignmentStatus::Unchanged),
        Some(current) if !transfer => return Err(AssignmentError::AlreadyAssigned(current)),
        Some(_) => AssignmentStatus::Transferred,
        None => AssignmentStatus::Assigned,
    };
    tx.mutate(move |sql| {
        Box::pin(async move {
            insert_assignment(sql, tenant, company_id, current).await?;
            Ok((
                (),
                vec![Change {
                    kind: "company.primary".into(),
                    id: company_id,
                    old: json!({"tenant_id":tenant,"company_id":current}),
                    new: json!({"tenant_id":tenant,"company_id":company_id}),
                }],
            ))
        })
    })
    .await?;
    Ok(status)
}
async fn insert_assignment(
    sql: &Transaction<'_>,
    tenant: i64,
    company_id: i64,
    current: Option<i64>,
) -> Result<()> {
    if current.is_some() {
        sql.execute(
            "UPDATE tenant_primary_companies SET company_id=$2 WHERE tenant_id=$1",
            &[&tenant, &company_id],
        )
        .await?;
    } else {
        sql.execute(
            "INSERT INTO tenant_primary_companies(tenant_id,company_id) VALUES($1,$2)",
            &[&tenant, &company_id],
        )
        .await?;
    }
    Ok(())
}

/// What a provisioning command produced.
#[derive(Clone, Debug)]
pub struct Provisioned {
    pub tenant: TenantIdentity,
    pub tenant_created: bool,
    pub company: Company,
    pub company_created: bool,
}
#[derive(Clone, Debug)]
pub enum ProvisionOutcome {
    Provisioned(Box<Provisioned>),
    Refused(Vec<FieldError>),
}

async fn provisioned_company(
    sql: &Transaction<'_>,
    tenant: i64,
    company: &NewCompany,
) -> Result<std::result::Result<Company, Vec<FieldError>>> {
    // Provisioning has no reader, so no field is restricted.
    let company = match validate(company, &HashSet::new()) {
        Ok(company) => company,
        Err(errors) => return Ok(Err(errors)),
    };
    lock_code(sql, tenant, &company.code).await?;
    let errors = check_references(sql, tenant, &company, None).await?;
    if !errors.is_empty() {
        return Ok(Err(errors));
    }
    Ok(Ok(insert(sql, tenant, &company).await?))
}
async fn record(
    sql: &Transaction<'_>,
    tenant: i64,
    event: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let mut who = Attribution::system(Some("provision"))?;
    who.tenant = Some(tenant);
    record_action(sql, &who, None, event, payload, true).await?;
    Ok(())
}

/// Atomically provisions a tenant and its first, primary company. Ids are
/// generated by PostgreSQL; this never accepts or assigns semantic numeric
/// ids. A deployment write: it records one retained
/// `company.tenant.provisioned` action as the `provision` system principal.
pub async fn provision_tenant(
    db: &Db,
    tenant_name: &str,
    company: &NewCompany,
) -> Result<ProvisionOutcome> {
    if tenant_name.trim().is_empty() {
        return Ok(ProvisionOutcome::Refused(vec![FieldError::new(
            "tenant_name",
            "can't be blank",
        )]));
    }
    let mut client = db.get().await?;
    let tx = client.transaction().await?;
    let sql: &Transaction<'_> = &tx;
    let tenant = bilibi_tenancy::admin::provision_tenant(sql, tenant_name).await?;
    let created = match provisioned_company(sql, tenant.id, company).await? {
        Ok(created) => created,
        Err(errors) => {
            tx.rollback().await?;
            return Ok(ProvisionOutcome::Refused(errors));
        }
    };
    insert_assignment(sql, tenant.id, created.id, None).await?;
    record(
        sql,
        tenant.id,
        "company.tenant.provisioned",
        json!({"summary":format!("Provisioned tenant “{}” on company “{}”",tenant.name,created.name),"subject":{"kind":"company","id":created.id,"name":created.name},"context":{"tenant_id":tenant.id,"tenant_name":tenant.name,"code":created.code}}),
    )
    .await?;
    tx.commit().await?;
    Ok(ProvisionOutcome::Provisioned(Box::new(Provisioned {
        tenant,
        tenant_created: true,
        company: created,
        company_created: true,
    })))
}

/// Provisions the explicit platform-operator tenant and its primary company.
/// Re-running resolves the existing relationship and creates neither a
/// second operator nor a second company.
pub async fn provision_platform_operator(
    db: &Db,
    tenant_name: &str,
    company: &NewCompany,
) -> Result<ProvisionOutcome> {
    let tenant_name = if tenant_name.trim().is_empty() {
        company.name.trim()
    } else {
        tenant_name.trim()
    };
    let mut client = db.get().await?;
    let tx = client.transaction().await?;
    let sql: &Transaction<'_> = &tx;
    let (tenant, tenant_created) =
        bilibi_tenancy::admin::provision_platform_operator(sql, tenant_name).await?;
    tx.execute(
        "SELECT id FROM tenants WHERE id=$1 FOR UPDATE",
        &[&tenant.id],
    )
    .await?;
    let existing: Option<i64> = tx
        .query_opt(
            "SELECT company_id FROM tenant_primary_companies WHERE tenant_id=$1 FOR UPDATE",
            &[&tenant.id],
        )
        .await?
        .map(|row| row.get(0));
    let (created, company_created) = match existing {
        Some(company_id) => {
            let row = tx
                .query_opt(
                    &format!(
                        "SELECT {} FROM companies WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL",
                        crate::organisation::COLUMNS
                    ),
                    &[&tenant.id, &company_id],
                )
                .await?
                .ok_or(Error::Internal)?;
            (Company::from_row(&row, &HashSet::new()), false)
        }
        None => {
            let created = match provisioned_company(sql, tenant.id, company).await? {
                Ok(created) => created,
                Err(errors) => {
                    tx.rollback().await?;
                    return Ok(ProvisionOutcome::Refused(errors));
                }
            };
            insert_assignment(sql, tenant.id, created.id, None).await?;
            (created, true)
        }
    };
    if tenant_created || company_created {
        record(
            sql,
            tenant.id,
            "company.platform_operator.provisioned",
            json!({"summary":format!("Provisioned platform operator “{}” on company “{}”",tenant.name,created.name),"subject":{"kind":"company","id":created.id,"name":created.name},"context":{"tenant_id":tenant.id,"tenant_created":tenant_created,"company_created":company_created}}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(ProvisionOutcome::Provisioned(Box::new(Provisioned {
        tenant,
        tenant_created,
        company: created,
        company_created,
    })))
}

impl From<AssignmentError> for WriteError {
    fn from(error: AssignmentError) -> Self {
        match error {
            AssignmentError::TenantNotFound | AssignmentError::CompanyNotFound => Self::NotFound,
            AssignmentError::CompanyArchived => Self::Archived,
            AssignmentError::AlreadyAssigned(current) => Self::Invalid(vec![FieldError::new(
                "company_id",
                format!("the tenant already stands on company {current}"),
            )]),
            AssignmentError::Access(error) => Self::Access(error),
        }
    }
}
