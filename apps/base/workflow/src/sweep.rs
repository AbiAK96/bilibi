//! Structural hygiene uses system identity and an opt-in owner locking seam.
//! It runs no owner business hooks and preserves owner -> run -> work lock order.
use super::*;
impl Engine {
    pub(crate) async fn sweep_processes(&self, limit: i64) -> Result<(usize, usize)> {
        let mut cursor = 0_i64;
        let client = self.db.get().await?;
        let horizon: i64 = client
            .query_one(
                "SELECT coalesce(max(id),0) FROM base_workflow_process_runs",
                &[],
            )
            .await?
            .get(0);
        drop(client);
        let (mut reconciled, mut skipped) = (0, 0);
        // Bound each page, but visit every running row once per sweep so a
        // permanently unavailable first page cannot starve later runs.
        loop {
            let client = self.db.get().await?;
            let rows=client.query("SELECT id,tenant_id FROM base_workflow_process_runs WHERE status='running' AND id>$1 AND id<=$3 ORDER BY id LIMIT $2",&[&cursor,&limit,&horizon]).await?;
            let count = rows.len();
            drop(client);
            for row in rows {
                cursor = row.get(0);
                let engine = self.clone();
                let tenant = row.get(1);
                let mut tasks = tokio::task::JoinSet::new();
                tasks.spawn(async move { engine.maintain_process(cursor, tenant).await });
                let result = match tasks.join_next().await {
                    Some(Ok(result)) => result,
                    _ => {
                        eprintln!("workflow maintenance skipped run {cursor}: owner panicked");
                        skipped += 1;
                        continue;
                    }
                };
                match result {
                    Ok(()) => reconciled += 1,
                    Err(
                        Error::Invalid
                        | Error::Conflict
                        | Error::NotFound
                        | Error::Forbidden
                        | Error::Unauthorized,
                    ) => {
                        eprintln!("workflow maintenance skipped run {cursor}: owner or definition unavailable");
                        skipped += 1;
                    }
                    Err(error) => return Err(error),
                }
            }
            if count < limit as usize {
                break;
            }
        }
        Ok((reconciled, skipped))
    }
    async fn maintain_process(&self, id: i64, tenant: i64) -> Result<()> {
        let mut client = self.db.get().await?;
        let scope = bilibi_authz::SystemScope::rehydrate(&**client, tenant, None).await?;
        let mut tx = self
            .boundary
            .begin(
                &mut client,
                scope.mutation_context("workflow.process.maintained"),
            )
            .await?;
        let engine = self.clone();
        tx.mutate(move |sql|Box::pin(async move {
            let candidate=sql.query_opt(&format!("SELECT {RUN_COLUMNS} FROM base_workflow_process_runs WHERE id=$1 AND tenant_id=$2"),&[&id,&tenant]).await?.map(run_row).ok_or(Error::NotFound)?;
            let owner=engine.registry.owned_subject(&candidate.subject_type)?;
            if candidate.owner!=owner.owner {return Err(Error::Invalid);}
            let subject=owner.adapter.maintenance_load(sql,&scope,candidate.subject_id).await?;
            if subject.tenant!=tenant || subject.id!=candidate.subject_id || !crate::definitions::key(&subject.status) || subject.version<0 || !subject.facts.is_object() {return Err(Error::Invalid);}
            let run=sql.query_opt(&format!("SELECT {RUN_COLUMNS} FROM base_workflow_process_runs WHERE id=$1 AND tenant_id=$2 FOR UPDATE"),&[&id,&tenant]).await?.map(run_row).ok_or(Error::NotFound)?;
            if run.subject_type!=candidate.subject_type || run.subject_id!=candidate.subject_id || run.owner!=candidate.owner {return Err(Error::Conflict);}
            let definition=engine.supported(sql,&run,owner).await?;
            let (items,dependencies)=graph(sql,&run,&definition,true).await?;
            let mut changes=Vec::new();
            settle(sql,&run,items,&dependencies,&mut changes).await?;
            changes.push(Change {kind:"workflow.process_maintenance".into(),id,old:json!({}),new:json!({"tenant":tenant})});
            Ok(((),changes))
        })).await?;
        tx.commit().await
    }
}
