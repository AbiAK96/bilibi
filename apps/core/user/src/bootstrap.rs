//! The bootstrap administrator's company (Bilimbi's `AdminBootstrap` creates
//! `ai@agent.my` under the operator's primary company). An account names a
//! company only through an affiliation of its Person, so the platform's
//! first administrator gets an agent Person under its own name, affiliated
//! with the company its bootstrap receipt names, and signs in under it.
//!
//! This happens once per receipt: `affiliation_settled_at` records it, and a
//! settled receipt is never revisited, so an operator who later unlinks,
//! re-links or retires that identity keeps the change. A receipt whose
//! account already names a Person is settled without a write. The writes
//! are the administrator's own (the receipt names the account, and nobody
//! else exists yet), on one audited mutation transaction.
use crate::{identity::LinkError, UserService};
use bilibi_affiliation::{AffiliationInput, EffectiveDates, OrganisationId};
use bilibi_database::{Error, Result};
use bilibi_person::{PersonInput, PersonKind};
use tokio_postgres::GenericClient;

/// What [`UserService::settle_bootstrap_affiliation`] did.
#[derive(Debug)]
pub enum BootstrapAffiliation {
    /// No receipt yet: nothing was bootstrapped.
    NoReceipt,
    /// The receipt was settled before; nothing was read further.
    Settled,
    /// The account already named a Person; the receipt is now settled.
    AlreadyLinked,
    /// The account named nothing and still cannot be bound; left unsettled
    /// so a later run tries again. The reason is for the operator.
    Deferred(&'static str),
    /// A Person and affiliation were created and the account signs in under
    /// the receipt's company.
    Linked { person: i64, affiliation: i64 },
}

async fn settled(client: &impl GenericClient) -> Result<Option<(i64, i64, i64, bool)>> {
    Ok(client
        .query_opt(
            "SELECT tenant_id, user_id, company_id, affiliation_settled_at IS NOT NULL FROM user_bootstrap",
            &[],
        )
        .await?
        .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3))))
}

impl UserService {
    /// Binds the bootstrap administrator to the receipt's company once; see
    /// the module documentation. Concurrent runs serialize on the receipt.
    pub async fn settle_bootstrap_affiliation(&self) -> Result<BootstrapAffiliation> {
        let mut client = self.db.get().await?;
        let Some((tenant, user, company, done)) = settled(&**client).await? else {
            return Ok(BootstrapAffiliation::NoReceipt);
        };
        if done {
            return Ok(BootstrapAffiliation::Settled);
        }
        let scope = match self
            .authentication
            .authenticate(&**client, tenant, user, String::new())
            .await
        {
            Ok(scope) => scope,
            Err(Error::Unauthorized) => {
                return Ok(BootstrapAffiliation::Deferred(
                    "the bootstrap administrator cannot sign in",
                ))
            }
            Err(error) => return Err(error),
        };
        let mut tx = self
            .mutations
            .begin(
                &mut client,
                scope.mutation_context("user.bootstrap.affiliation"),
            )
            .await?;
        let locked = tx
            .sql()
            .query_one(
                "SELECT affiliation_settled_at IS NOT NULL FROM user_bootstrap FOR UPDATE",
                &[],
            )
            .await?;
        if locked.get::<_, bool>(0) {
            tx.rollback().await?;
            return Ok(BootstrapAffiliation::Settled);
        }
        if scope.actor().person().is_some() {
            mark_settled(tx.sql()).await?;
            tx.commit().await?;
            return Ok(BootstrapAffiliation::AlreadyLinked);
        }
        let person = match bilibi_person::identity::create(
            &mut tx,
            &scope,
            &PersonInput {
                name: scope.user_name.clone(),
                kind: PersonKind::Agent,
                email: None,
                phone: None,
            },
        )
        .await
        {
            Ok(person) => person.id,
            Err(bilibi_person::WriteError::Backend(Error::Forbidden)) => {
                return Ok(BootstrapAffiliation::Deferred(
                    "the bootstrap administrator may no longer create contacts",
                ))
            }
            Err(bilibi_person::WriteError::Backend(error)) => return Err(error),
            Err(_) => return Err(Error::Invalid),
        };
        let organisation = OrganisationId::new(company).map_err(|_| Error::Internal)?;
        let affiliation = match bilibi_affiliation::identity::create(
            &mut tx,
            &scope,
            &AffiliationInput {
                person_id: person,
                organisation_id: organisation,
                title: None,
                work_email: None,
                work_phone: None,
                dates: EffectiveDates::new(None, None).map_err(|_| Error::Internal)?,
            },
        )
        .await
        {
            Ok(affiliation) => affiliation.id,
            Err(Error::Forbidden | Error::NotFound) => {
                return Ok(BootstrapAffiliation::Deferred(
                    "the bootstrap company is gone or read-only, or the administrator may no longer affiliate contacts",
                ))
            }
            Err(error) => return Err(error),
        };
        match self
            .link_affiliation_in(&mut tx, &scope, user, affiliation)
            .await
        {
            Ok(_) => {}
            Err(LinkError::Access(Error::Unauthorized)) => return Err(Error::Unauthorized),
            Err(LinkError::Access(error @ (Error::Database(_) | Error::Pool(_)))) => {
                return Err(error)
            }
            Err(_) => {
                return Ok(BootstrapAffiliation::Deferred(
                    "the bootstrap administrator's account cannot be linked",
                ))
            }
        }
        mark_settled(tx.sql()).await?;
        tx.commit().await?;
        Ok(BootstrapAffiliation::Linked {
            person: person.get(),
            affiliation: affiliation.get(),
        })
    }
}

async fn mark_settled(sql: &impl GenericClient) -> Result<()> {
    sql.execute(
        "UPDATE user_bootstrap SET affiliation_settled_at = now()",
        &[],
    )
    .await?;
    Ok(())
}
