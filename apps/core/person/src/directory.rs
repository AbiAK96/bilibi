use bilibi_database::{Error, Result};
use bilibi_principal_directory::{
    DirectoryClient, DirectoryFuture, PrincipalKind, PrincipalProvider,
};
use bilibi_tenancy::Scope;
use std::collections::HashMap;

pub struct Provider(pub PrincipalKind);
impl PrincipalProvider for Provider {
    fn kind(&self) -> PrincipalKind {
        self.0
    }
    fn names<'a>(
        &'a self,
        client: &'a dyn DirectoryClient,
        scope: &'a Scope,
        ids: &'a [i64],
    ) -> DirectoryFuture<'a, HashMap<i64, String>> {
        Box::pin(async move {
            bounded(ids)?;
            let agents_only = self.0 == PrincipalKind::Agent;
            let rows = client.query("SELECT id,name FROM persons WHERE tenant_id=$1 AND id=ANY($2) AND retired_at IS NULL AND (NOT $3 OR kind='agent')", &[&scope.tenant(), &ids, &agents_only]).await?;
            Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
        })
    }
    // Personal contact matching stays in Person's authorized API; Base can name
    // its bounded referenced principals but cannot probe their hidden contacts.
}
fn bounded(ids: &[i64]) -> Result<()> {
    if ids.len() > bilibi_principal_directory::MAX_CEILING {
        Err(Error::Invalid)
    } else {
        Ok(())
    }
}
