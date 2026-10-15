//! Installation-locale facts from the platform operator's primary company
//! address (Bilimbi `Address.platform_operator_locale_bootstrap/0`). Base
//! Locale never queries Core; whoever resolves the locale (K15) asks here and
//! passes the `Bootstrap` along.
use bilibi_database::{Error, Result};
use bilibi_geonames::Geonames;
use bilibi_locale::Bootstrap;
use tokio_postgres::GenericClient;

/// `None` unless a live platform-operator tenant stands on a primary company
/// whose first address (primary, then priority, then id) has a country.
/// A missing operator, primary company or country is an ordinary `None`; a
/// broken primary-company assignment is also `None`, as in Bilimbi, so a
/// locale read never fails on it.
pub async fn platform_operator(
    client: &impl GenericClient,
    geonames: &Geonames,
) -> Result<Option<Bootstrap>> {
    let Some(tenant) = client
        .query_opt(
            "SELECT o.tenant_id FROM platform_operator o JOIN tenants t ON t.id=o.tenant_id WHERE o.singleton AND t.status='active' AND t.deleted_at IS NULL",
            &[],
        )
        .await?
        .map(|row| row.get::<_, i64>(0))
    else {
        return Ok(None);
    };
    let company = match bilibi_company::primary::primary_company(client, tenant).await {
        Ok(Some(company)) => company,
        Ok(None) | Err(Error::Internal) => return Ok(None),
        Err(error) => return Err(error),
    };
    let country: Option<String> = client
        .query_opt(
            "SELECT a.country_iso FROM address_attachments t JOIN addresses a ON a.tenant_id=t.tenant_id AND a.id=t.address_id \
             WHERE t.tenant_id=$1 AND t.organisation_id=$2 AND a.deleted_at IS NULL \
             ORDER BY t.is_primary DESC, t.priority, a.id LIMIT 1",
            &[&tenant, &company.id.get()],
        )
        .await?
        .and_then(|row| row.get(0));
    let Some(Ok(mut facts)) = country.as_deref().map(Bootstrap::new) else {
        return Ok(None);
    };
    if let Some(country) = geonames.get_country(facts.country_iso()).await? {
        facts.country_name = Some(country.country);
        facts.languages = country.languages;
        facts.currency_code = country.currency_code;
    }
    Ok(Some(facts))
}
