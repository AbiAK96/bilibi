//! Graph lifecycle writes. The subject lock precedes the run and graph locks;
//! every policy check, work fact and event shares the caller's audited commit.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub expected_version: i64,
    pub executor_key: String,
    pub outcome: String,
    pub output: Value,
    pub result_ref: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunCommand {
    /// Release due timers and propagate impossible dependency outcomes.
    Reconcile,
    Complete {
        step_key: String,
        completion: Completion,
    },
    Signal {
        name: String,
        payload: Value,
        idempotency_key: String,
    },
    Pause {
        reason: String,
    },
    Resume,
    /// Preserve terminal work and block every unfinished item. Live leases refuse.
    Supersede {
        reason: String,
    },
}
impl RunCommand {
    fn operation(&self) -> Operation {
        match self {
            Self::Reconcile => Operation::ReconcileRun,
            Self::Complete { .. } => Operation::CompleteWork,
            Self::Signal { .. } => Operation::SignalRun,
            Self::Pause { .. } => Operation::PauseRun,
            Self::Resume => Operation::ResumeRun,
            Self::Supersede { .. } => Operation::SupersedeRun,
        }
    }
    fn action(&self) -> &'static str {
        match self {
            Self::Reconcile => "workflow.process.reconciled",
            Self::Complete { .. } => "workflow.work.completed",
            Self::Signal { .. } => "workflow.signal.received",
            Self::Pause { .. } => "workflow.process.paused",
            Self::Resume => "workflow.process.resumed",
            Self::Supersede { .. } => "workflow.process.superseded",
        }
    }
    fn validate(&self) -> Result<()> {
        use crate::process::text;
        let valid = match self {
            Self::Complete {
                step_key,
                completion: c,
            } => {
                crate::definitions::key(step_key)
                    && c.expected_version >= 1
                    && text(&c.executor_key)
                    && text(&c.outcome)
                    && c.result_ref.as_deref().is_none_or(text)
            }
            Self::Signal {
                name,
                idempotency_key,
                ..
            } => text(name) && text(idempotency_key),
            Self::Pause { reason } | Self::Supersede { reason } => {
                !reason.trim().is_empty() && !reason.contains('\0') && reason.len() <= 16000
            }
            _ => true,
        };
        if !valid || serde_json::to_vec(self).map_err(|_| Error::Invalid)?.len() > 65536 {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
impl Engine {
    pub async fn update_run(
        &self,
        scope: &Scope,
        run_id: i64,
        command: RunCommand,
    ) -> Result<RunView> {
        let mut client = self.db.get().await?;
        let mut tx = self
            .boundary
            .begin(&mut client, scope.mutation_context(command.action()))
            .await?;
        let result = self.update_run_in(&mut tx, scope, run_id, command).await?;
        tx.commit().await?;
        Ok(result)
    }
    /// Owner facades use this to commit their business writes with completion.
    /// A refusal or cancellation poisons the entire enclosing transaction.
    /// `ReadRun` never authorizes these operations: both owner adapters receive
    /// the specific command operation after live authentication and locking.
    pub async fn update_run_in(
        &self,
        tx: &mut MutationTransaction<'_>,
        scope: &Scope,
        run_id: i64,
        command: RunCommand,
    ) -> Result<RunView> {
        let context = tx.context().clone();
        let engine = self.clone();
        let scope = scope.clone();
        tx.mutate(move |sql| {
            Box::pin(async move {
                command.validate()?;
                engine.writer(&context, &scope, run_id)?;
                let scope = engine.live(sql, &scope).await?;
                let (run, definition, items, dependencies) = engine
                    .locked_run(sql, &scope, run_id, command.operation())
                    .await?;
                let mut changes = Vec::new();
                apply(sql, &run, items, &dependencies, &command, &mut changes).await?;
                let run = sql
                    .query_one(
                        &format!(
                            "SELECT {RUN_COLUMNS} FROM base_workflow_process_runs WHERE id=$1"
                        ),
                        &[&run.id],
                    )
                    .await
                    .map(run_row)?;
                let (items, _) = graph(sql, &run, &definition, false).await?;
                // Idempotent commands still record an authorized attempt, without
                // appending duplicate lifecycle events or rewriting completed work.
                changes.push(Change {
                    kind: "workflow.process_command".into(),
                    id: run.id,
                    old: json!({}),
                    new: json!({"action":command.action(),"status":run.status}),
                });
                Ok((
                    RunView {
                        run,
                        work_items: items.into_iter().map(|i| i.fact).collect(),
                    },
                    changes,
                ))
            })
        })
        .await
    }
    async fn locked_run(
        &self,
        sql: &Transaction<'_>,
        scope: &Scope,
        run_id: i64,
        operation: Operation,
    ) -> Result<(Run, ProcessDefinition, Vec<Item>, Vec<Dependency>)> {
        let candidate = sql.query_opt(&format!("SELECT {RUN_COLUMNS} FROM base_workflow_process_runs WHERE id=$1 AND tenant_id=$2"), &[&run_id, &scope.tenant()]).await?.map(run_row).ok_or(Error::NotFound)?;
        let owner = self.registry.owned_subject(&candidate.subject_type)?;
        if candidate.owner != owner.owner {
            return Err(Error::Invalid);
        }
        let subject = self
            .load(
                sql,
                scope,
                owner,
                candidate.subject_id,
                Load::Lock,
                operation,
            )
            .await?;
        let run = sql.query_opt(&format!("SELECT {RUN_COLUMNS} FROM base_workflow_process_runs WHERE id=$1 AND tenant_id=$2 FOR UPDATE"), &[&run_id, &scope.tenant()]).await?.map(run_row).ok_or(Error::NotFound)?;
        if run.subject_type != candidate.subject_type
            || run.subject_id != candidate.subject_id
            || run.owner != candidate.owner
        {
            return Err(Error::Conflict);
        }
        let definition = self.supported(sql, &run, owner).await?;
        definition
            .adapter
            .authorize(sql, scope, &subject, &facts(&run), operation)
            .await?;
        let (items, dependencies) = graph(sql, &run, &definition, true).await?;
        Ok((run, definition, items, dependencies))
    }
}
async fn apply(
    sql: &Transaction<'_>,
    run: &Run,
    items: Vec<Item>,
    dependencies: &[Dependency],
    command: &RunCommand,
    changes: &mut Vec<Change>,
) -> Result<()> {
    match command {
        RunCommand::Reconcile => settle(sql, run, items, dependencies, changes).await,
        RunCommand::Complete {
            step_key,
            completion,
        } => {
            let item = items
                .iter()
                .find(|i| i.fact.step_key == *step_key)
                .ok_or(Error::NotFound)?;
            let due: bool = sql.query_one("SELECT r.available_at<=now() AND w.available_at<=now() FROM base_workflow_process_runs r JOIN base_workflow_process_work_items w ON w.run_id=r.id WHERE r.id=$1 AND w.id=$2", &[&run.id, &item.fact.id]).await?.get::<_, Option<bool>>(0).unwrap_or(false);
            if run.status != "running"
                || run.last_error.is_some()
                || item.fact.status != "available"
                || !due
                || item.fact.executor_key != completion.executor_key
                || item.fact.version != completion.expected_version
            {
                return Err(Error::Conflict);
            }
            let result_ref = completion
                .result_ref
                .as_ref()
                .or(item.fact.result_ref.as_ref());
            sql.execute("UPDATE base_workflow_process_work_items SET status='completed',outcome=$2,output=$3,result_ref=$4,last_error=NULL,lease_owner=NULL,lease_token=NULL,lease_expires_at=NULL,heartbeat_at=NULL,completed_at=now(),updated_at=now(),version=version+1 WHERE id=$1", &[&item.fact.id, &completion.outcome, &completion.output, &result_ref]).await?;
            changes.push(Change {kind:"workflow.process_work".into(),id:item.fact.id,old:json!({"status":item.fact.status,"version":item.fact.version}),new:json!({"status":"completed","outcome":completion.outcome,"output":completion.output,"result_ref":result_ref,"version":item.fact.version+1})});
            append_event(sql, run, Some(item.fact.id), "work.completed", json!({"outcome":completion.outcome,"output":completion.output,"result_ref":result_ref}), changes).await?;
            let items = load_items(sql, run, items.len() as i64, false).await?;
            settle(sql, run, items, dependencies, changes).await
        }
        RunCommand::Signal {
            name,
            payload,
            idempotency_key,
        } => {
            let key = event_key(format!("signal:{}:{idempotency_key}", run.id));
            let expected = json!({"signal":name,"payload":payload});
            if let Some(previous) = sql.query_opt("SELECT type,payload FROM base_workflow_process_events WHERE run_id=$1 AND tenant_id=$2 AND idempotency_key=$3", &[&run.id, &run.tenant, &key]).await? {
                if previous.get::<_, String>(0) == "signal.received" && previous.get::<_, Option<Value>>(1).as_ref() == Some(&expected) {
                    return Ok(());
                }
                return Err(Error::Conflict);
            }
            if run.status != "running" {
                return Err(Error::Conflict);
            }
            for item in &items {
                if item.fact.required_signal.as_ref() == Some(name)
                    && item.fact.signalled_at.is_none()
                    && !TERMINAL_WORK.contains(&item.fact.status.as_str())
                {
                    sql.execute("UPDATE base_workflow_process_work_items SET signalled_at=now(),signal_payload=$2,updated_at=now() WHERE id=$1", &[&item.fact.id, payload]).await?;
                    changes.push(Change {
                        kind: "workflow.process_work".into(),
                        id: item.fact.id,
                        old: json!({"signalled_at":null}),
                        new: json!({"signal":name,"signal_payload":payload}),
                    });
                }
            }
            append_keyed_event(
                sql,
                run,
                None,
                "signal.received",
                expected,
                Some(&key),
                changes,
            )
            .await?;
            let items = load_items(sql, run, items.len() as i64, false).await?;
            settle(sql, run, items, dependencies, changes).await
        }
        RunCommand::Pause { reason } => {
            if run.status == "paused" {
                return Ok(());
            }
            if run.status != "running" {
                return Err(Error::Conflict);
            }
            sql.execute("UPDATE base_workflow_process_runs SET status='paused',paused_at=now(),pause_reason=$2,heartbeat_at=now(),updated_at=now() WHERE id=$1", &[&run.id, reason]).await?;
            changes.push(Change {
                kind: "workflow.process_run".into(),
                id: run.id,
                old: json!({"status":run.status}),
                new: json!({"status":"paused","pause_reason":reason}),
            });
            append_event(
                sql,
                run,
                None,
                "process.paused",
                json!({"reason":reason}),
                changes,
            )
            .await?;
            Ok(())
        }
        RunCommand::Resume => {
            if run.status == "running" {
                return Ok(());
            }
            if run.status != "paused" {
                return Err(Error::Conflict);
            }
            sql.execute("UPDATE base_workflow_process_runs SET status='running',paused_at=NULL,pause_reason=NULL,heartbeat_at=now(),updated_at=now() WHERE id=$1", &[&run.id]).await?;
            changes.push(Change {
                kind: "workflow.process_run".into(),
                id: run.id,
                old: json!({"status":run.status}),
                new: json!({"status":"running"}),
            });
            append_event(sql, run, None, "process.resumed", Value::Null, changes).await?;
            let mut running = run.clone();
            running.status = "running".into();
            settle(sql, &running, items, dependencies, changes).await
        }
        RunCommand::Supersede { reason } => {
            if !matches!(run.status.as_str(), "running" | "paused") {
                return Ok(());
            }
            let leased: bool = sql.query_one("SELECT EXISTS(SELECT 1 FROM base_workflow_process_work_items WHERE run_id=$1 AND status='leased' AND lease_expires_at>now())", &[&run.id]).await?.get(0);
            if leased {
                return Err(Error::Conflict);
            }
            for item in &items {
                if TERMINAL_WORK.contains(&item.fact.status.as_str()) {
                    continue;
                }
                sql.execute("UPDATE base_workflow_process_work_items SET status='blocked',outcome='blocked',output=NULL,last_error=$2,lease_owner=NULL,lease_token=NULL,lease_expires_at=NULL,heartbeat_at=NULL,completed_at=now(),updated_at=now(),version=version+1 WHERE id=$1", &[&item.fact.id, reason]).await?;
                changes.push(Change {kind:"workflow.process_work".into(),id:item.fact.id,old:json!({"status":item.fact.status,"version":item.fact.version}),new:json!({"status":"blocked","outcome":"blocked","reason":reason,"version":item.fact.version+1})});
                append_event(
                    sql,
                    run,
                    Some(item.fact.id),
                    "work.blocked",
                    json!({"outcome":"blocked","reason":reason,"cause":"process_superseded"}),
                    changes,
                )
                .await?;
            }
            let items = load_items(sql, run, items.len() as i64, false).await?;
            finish_run(sql, run, &items, changes).await?;
            append_event(
                sql,
                run,
                None,
                "process.superseded",
                json!({"reason":reason}),
                changes,
            )
            .await?;
            Ok(())
        }
    }
}
