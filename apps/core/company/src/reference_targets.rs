//! Company reference targets for tenant-defined fields, mirroring Bilimbi's
//! scoped company directory used by Company pickers.
use bilibi_custom_fields::{ReferenceFuture, ReferenceOption, ReferenceTargetProvider};
use bilibi_party_ids::OrganisationId;
use bilibi_tenancy::{OrganisationDirectory, ReadClient, Scope};

pub(crate) struct Companies;
pub(crate) static COMPANIES: Companies = Companies;

impl ReferenceTargetProvider for Companies {
    fn choices<'a>(
        &'a self,
        client: &'a dyn ReadClient,
        scope: &'a Scope,
        search: &'a str,
        limit: usize,
    ) -> ReferenceFuture<'a, Vec<ReferenceOption>> {
        Box::pin(async move {
            let companies =
                crate::organisation::searchable_reference_targets(client, scope, search, limit)
                    .await?;
            Ok(companies
                .into_iter()
                .filter(|company| !company.archived)
                .map(|company| ReferenceOption {
                    key: company.id.get().to_string(),
                    label: company.name,
                })
                .collect())
        })
    }

    fn resolve<'a>(
        &'a self,
        client: &'a dyn ReadClient,
        scope: &'a Scope,
        key: &'a str,
    ) -> ReferenceFuture<'a, Option<String>> {
        Box::pin(async move {
            let Some(id) = key
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .and_then(|id| OrganisationId::new(id).ok())
            else {
                return Ok(None);
            };
            match crate::organisation::Directory
                .target(client, scope, id, crate::VIEW)
                .await
            {
                Ok(company) if !company.archived => Ok(Some(company.name)),
                Ok(_)
                | Err(bilibi_database::Error::Forbidden | bilibi_database::Error::NotFound) => {
                    Ok(None)
                }
                Err(error) => Err(error),
            }
        })
    }

    fn resolve_many<'a>(
        &'a self,
        client: &'a dyn ReadClient,
        scope: &'a Scope,
        keys: &'a [String],
    ) -> ReferenceFuture<'a, Vec<ReferenceOption>> {
        Box::pin(async move {
            let ids = keys
                .iter()
                .filter_map(|key| key.parse::<i64>().ok().filter(|id| *id > 0))
                .collect::<Vec<_>>();
            let companies =
                crate::organisation::readable_reference_targets(client, scope, &ids).await?;
            Ok(companies
                .into_iter()
                .map(|company| ReferenceOption {
                    key: company.id.get().to_string(),
                    label: company.name,
                })
                .collect())
        })
    }
}
