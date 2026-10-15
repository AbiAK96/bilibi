//! Operator-driven private owner specimen. Commands execute real PostgreSQL
//! transactions; this is not a unit-test runner or an application route.
use bilibi_database::{pool, Change, Error, MutationBoundary, Result};
use bilibi_module_registry::{Contribution as Registration, Provenance, Services};
use bilibi_tenancy::{Authentication, Scope};
use bilibi_workflow::{
    Action, ActionRequest, ActionResult, Claim, ClaimOptions, Completion, Contribution, Dependency,
    DependencyMode, Edge, Engine, Flow, Guard, HumanAction, HumanActionHandler, HumanOutcome, Load,
    Operation, OwnerFuture, Process, ProcessAdapter, Registry, Request, RunCommand, RunFacts,
    RunView, StartOptions, Status, Step, Subject, SubjectAdapter, Transition, WorkCommand,
    WorkView, CONTRIBUTIONS,
};
use serde_json::json;
use std::sync::Arc;
use tokio_postgres::Transaction;
struct Owner;
impl SubjectAdapter for Owner {
    fn load<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        id: i64,
        mode: Load,
    ) -> OwnerFuture<'a, Subject> {
        Box::pin(async move {
            let sql=match mode { Load::Read=>"SELECT status,version FROM workflow_reference_records WHERE tenant_id=$1 AND id=$2",Load::Lock=>"SELECT status,version FROM workflow_reference_records WHERE tenant_id=$1 AND id=$2 FOR UPDATE"};
            let row = tx
                .query_opt(sql, &[&scope.tenant(), &id])
                .await?
                .ok_or(Error::NotFound)?;
            Ok(Subject {
                id,
                tenant: scope.tenant(),
                status: row.get(0),
                version: row.get(1),
                facts: json!({}),
            })
        })
    }
    fn authorize<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        _: &'a Subject,
        operation: Operation,
    ) -> OwnerFuture<'a, ()> {
        Box::pin(async move {
            bilibi_authz::authorize_key(
                tx,
                scope,
                if matches!(operation, Operation::Read | Operation::ReadRun) {
                    "companies.view"
                } else {
                    "companies.update"
                },
            )
            .await
        })
    }
    fn persist<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        subject: &'a Subject,
        to: &'a str,
        request: &'a Request,
    ) -> OwnerFuture<'a, Vec<Change>> {
        Box::pin(async move {
            let status = if request.input["bad_persist"] == true {
                "pending"
            } else {
                to
            };
            let row=tx.query_opt("UPDATE workflow_reference_records SET status=$1,version=version+1 WHERE tenant_id=$2 AND id=$3 AND status=$4 AND version=$5 RETURNING version",&[&status,&scope.tenant(),&subject.id,&subject.status,&subject.version]).await?.ok_or(Error::Conflict)?;
            Ok(vec![Change {
                kind: "reference.record".into(),
                id: subject.id,
                old: json!({"status":subject.status,"version":subject.version}),
                new: json!({"status":status,"version":row.get::<_,i64>(0)}),
            }])
        })
    }
}
impl Guard for Owner {
    fn check<'a>(
        &'a self,
        _: &'a Transaction<'_>,
        _: &'a Scope,
        _: &'a Subject,
        _: &'a Transition,
        request: &'a Request,
    ) -> OwnerFuture<'a, ()> {
        Box::pin(async move {
            if request.input["guard_deny"] == true {
                Err(Error::Forbidden)
            } else {
                Ok(())
            }
        })
    }
}
impl Action for Owner {
    fn execute<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        subject: &'a Subject,
        _: &'a Transition,
        request: &'a Request,
    ) -> OwnerFuture<'a, Vec<Change>> {
        Box::pin(async move {
            tx.execute("UPDATE workflow_reference_records SET action_count=action_count+1 WHERE tenant_id=$1 AND id=$2",&[&scope.tenant(),&subject.id]).await?;
            if request.input["action_fail"] == true {
                return Err(Error::Invalid);
            }
            if request.input["action_wait"] == true {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
            Ok(vec![Change {
                kind: "reference.action".into(),
                id: subject.id,
                old: json!({"increment":0}),
                new: json!({"increment":1}),
            }])
        })
    }
}
impl ProcessAdapter for Owner {
    fn authorize<'a>(
        &'a self,
        _: &'a Transaction<'_>,
        _: &'a Scope,
        subject: &'a Subject,
        run: &'a RunFacts,
        _: Operation,
    ) -> OwnerFuture<'a, ()> {
        Box::pin(async move {
            // Owner policy: a run belongs to the locked subject; an archived
            // reference record would be refused here.
            if run.subject_id != subject.id || run.input.get("deny") == Some(&json!(true)) {
                return Err(Error::Forbidden);
            }
            Ok(())
        })
    }
}
impl HumanActionHandler for Owner {
    fn handle<'a>(
        &'a self,
        tx: &'a Transaction<'_>,
        scope: &'a Scope,
        subject: &'a Subject,
        action: &'a HumanAction,
        request: &'a ActionRequest,
    ) -> OwnerFuture<'a, HumanOutcome> {
        Box::pin(async move {
            // The owner write: every row change increments the version, so a
            // page holding the old version cannot act again.
            let row = tx.query_opt("UPDATE workflow_reference_records SET action_count=action_count+1,version=version+1 WHERE tenant_id=$1 AND id=$2 AND version=$3 RETURNING version,action_count", &[&scope.tenant(), &subject.id, &subject.version]).await?.ok_or(Error::Conflict)?;
            let (version, count): (i64, i32) = (row.get(0), row.get(1));
            if request.payload.get("handler_wait") == Some(&json!(true)) {
                // Widens the window so a concurrent duplicate really overlaps.
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            if request.payload.get("handler_fail") == Some(&json!(true)) {
                return Err(Error::Invalid);
            }
            Ok(HumanOutcome {
                outcome: "completed".into(),
                output: json!({"action":action.key,"action_count":count,"payload":request.payload}),
                result_ref: Some(format!("reference:{}:{}", subject.id, action.key)),
                changes: vec![Change {
                    kind: "reference.human_action".into(),
                    id: subject.id,
                    old: json!({"version":subject.version}),
                    new: json!({"version":version,"action_count":count}),
                }],
            })
        })
    }
}
static OWNER: Owner = Owner;
const REVIEW_V1: [Step; 3] = [
    Step::new("first", "First review"),
    Step::new("second", "Second review"),
    Step {
        dependencies: &[
            Dependency {
                step: "first",
                acceptable_outcomes: &[],
            },
            Dependency {
                step: "second",
                acceptable_outcomes: &["completed", "waived"],
            },
        ],
        ..Step::new("finish", "Finish")
    },
];
const REVIEW_V2: [Step; 4] = [
    REVIEW_V1[0],
    REVIEW_V1[1],
    REVIEW_V1[2],
    Step {
        dependencies: &[Dependency {
            step: "finish",
            acceptable_outcomes: &[],
        }],
        delay_seconds: 3600,
        ..Step::new("archive", "Archive")
    },
];
// A bounded graph for operating the lifecycle API against real durable facts.
const LIFECYCLE: [Step; 6] = [
    Step::new("first", "First review"),
    Step::new("second", "Second review"),
    Step {
        dependencies: &[
            Dependency {
                step: "first",
                acceptable_outcomes: &["accepted"],
            },
            Dependency {
                step: "second",
                acceptable_outcomes: &[],
            },
        ],
        ..Step::new("all", "Both reviews")
    },
    Step {
        dependencies: &[
            Dependency {
                step: "first",
                acceptable_outcomes: &["accepted"],
            },
            Dependency {
                step: "second",
                acceptable_outcomes: &[],
            },
        ],
        mode: DependencyMode::Any,
        ..Step::new("any", "Either review")
    },
    Step {
        dependencies: &[Dependency {
            step: "all",
            acceptable_outcomes: &[],
        }],
        required_signal: Some("review.approved"),
        ..Step::new("signal", "Approval signal")
    },
    Step {
        dependencies: &[
            Dependency {
                step: "any",
                acceptable_outcomes: &[],
            },
            Dependency {
                step: "signal",
                acceptable_outcomes: &[],
            },
        ],
        delay_seconds: 2,
        ..Step::new("delayed", "Delayed release")
    },
];
// Worker-leased steps: `fetch` allows two attempts, the others one.
const WORKER: [Step; 4] = [
    Step {
        max_attempts: 2,
        executor: Some("reference.fetch"),
        ..Step::new("fetch", "Fetch source")
    },
    Step {
        dependencies: &[Dependency {
            step: "fetch",
            acceptable_outcomes: &[],
        }],
        executor: Some("reference.render"),
        ..Step::new("render", "Render")
    },
    Step {
        dependencies: &[Dependency {
            step: "render",
            acceptable_outcomes: &["completed", "waived"],
        }],
        executor: Some("reference.publish"),
        ..Step::new("publish", "Publish")
    },
    Step {
        executor: Some("reference.notify"),
        ..Step::new("notify", "Notify")
    },
];
// A person submits `review` through a work-bound human action.
const HUMAN: [Step; 2] = [
    Step {
        executor: Some("reference.human.review"),
        ..Step::new("review", "Human review")
    },
    Step {
        dependencies: &[Dependency {
            step: "review",
            acceptable_outcomes: &[],
        }],
        executor: Some("reference.human.publish"),
        ..Step::new("publish", "Publish")
    },
];
/// Same key and version as the installed v1 with one changed step contract:
/// the running proof builds a second registry from it to show the refusal.
static DRIFTED_V1: Contribution = Contribution::Process(Process {
    key: "reference.review",
    version: 1,
    subject: "reference.record",
    adapter: &OWNER,
    steps: &[
        Step::new("first", "First review (renamed)"),
        REVIEW_V1[1],
        REVIEW_V1[2],
    ],
});
inventory::submit! { bilibi_module_registry::Module {
    id:"extensions/workflow-reference",layer:bilibi_module_registry::Layer::Extensions,
    dependencies:&["base/workflow"],services:&[],
    migrations:&[("workflow-reference-001","CREATE TABLE workflow_reference_records(id bigint PRIMARY KEY,tenant_id bigint NOT NULL REFERENCES tenants(id),status text NOT NULL DEFAULT 'pending',version bigint NOT NULL DEFAULT 0,action_count integer NOT NULL DEFAULT 0,facade_value integer NOT NULL DEFAULT 0)")],
} }
macro_rules! register {($id:literal,$value:expr)=>{inventory::submit! { Registration::new(CONTRIBUTIONS,$id,Provenance {module:"extensions/workflow-reference",source:file!(),line:line!()},&$value) }};}
register!(
    "reference.subject",
    Contribution::Subject {
        key: "reference.record",
        aliases: &["ReferenceRecord"],
        adapter: &OWNER
    }
);
register!(
    "reference.guard",
    Contribution::Guard {
        key: "reference.ready",
        aliases: &[],
        adapter: &OWNER
    }
);
register!(
    "reference.action",
    Contribution::Action {
        key: "reference.approved",
        aliases: &[],
        adapter: &OWNER
    }
);
register!(
    "reference.process.v1",
    Contribution::Process(Process {
        key: "reference.review",
        version: 1,
        subject: "reference.record",
        adapter: &OWNER,
        steps: &REVIEW_V1,
    })
);
register!(
    "reference.process.v2",
    Contribution::Process(Process {
        key: "reference.review",
        version: 2,
        subject: "reference.record",
        adapter: &OWNER,
        steps: &REVIEW_V2,
    })
);
register!(
    "reference.process.lifecycle",
    Contribution::Process(Process {
        key: "reference.lifecycle",
        version: 1,
        subject: "reference.record",
        adapter: &OWNER,
        steps: &LIFECYCLE,
    })
);
register!(
    "reference.process.worker",
    Contribution::Process(Process {
        key: "reference.worker",
        version: 1,
        subject: "reference.record",
        adapter: &OWNER,
        steps: &WORKER,
    })
);
register!(
    "reference.process.human",
    Contribution::Process(Process {
        key: "reference.human",
        version: 1,
        subject: "reference.record",
        adapter: &OWNER,
        steps: &HUMAN,
    })
);
register!(
    "reference.human.acknowledge",
    Contribution::HumanAction(HumanAction {
        key: "reference.acknowledge",
        label: "Acknowledge",
        subject: "reference.record",
        capability: "companies.update",
        executor: None,
        handler: &OWNER,
    })
);
register!(
    "reference.human.review",
    Contribution::HumanAction(HumanAction {
        key: "reference.review.submit",
        label: "Submit review",
        subject: "reference.record",
        capability: "companies.update",
        executor: Some("reference.human.review"),
        handler: &OWNER,
    })
);
register!(
    "reference.flow",
    Contribution::Flow(Flow {
        code: "reference.approval",
        label: "Reference approval",
        subject: "reference.record",
        active: true,
        statuses: &[
            Status {
                code: "pending",
                label: "Pending",
                position: 0,
                active: true,
                settings: "{}"
            },
            Status {
                code: "approved",
                label: "Approved",
                position: 1,
                active: true,
                settings: "{}"
            }
        ],
        edges: &[Edge {
            from: "pending",
            to: "approved",
            label: "Approve",
            capability: Some("companies.update"),
            guard: Some("reference.ready"),
            action: Some("reference.approved"),
            position: 0,
            active: true,
            sla_seconds: None,
            metadata: "{}"
        }],
    })
);

/// One line per run and work item: the facts an operator checks between commands.
fn print_view(view: &RunView) {
    let run = &view.run;
    println!(
        "run={} status={} definition={} v{} fingerprint={} input={} pause_reason={:?} output={}",
        run.id,
        run.status,
        run.definition_key,
        run.definition_version,
        run.definition_fingerprint,
        run.input,
        run.pause_reason,
        run.output.clone().unwrap_or(json!(null))
    );
    for item in &view.work_items {
        println!(
            "  work={} step={} status={} attempts={}/{} lease_expires_at={:?} outcome={:?} output={} signalled={} available_at={:?} version={} error={:?}",
            item.id,
            item.step_key,
            item.status,
            item.attempts,
            item.max_attempts,
            item.lease_expires_at,
            item.outcome,
            item.output.clone().unwrap_or(json!(null)),
            item.signalled_at.is_some(),
            item.available_at,
            item.version,
            item.last_error
        );
    }
}

/// The page's view of a human action: the first available entry for
/// `action_key`, with the subject and work versions it would echo back.
async fn page_request(
    engine: &Engine,
    scope: &Scope,
    id: i64,
    action_key: &str,
    idempotency_key: &str,
    payload: Option<&String>,
) -> Result<ActionRequest> {
    let available = engine
        .available_actions(scope, "reference.record", id)
        .await?;
    let entry = available
        .actions
        .iter()
        .find(|a| a.key == action_key)
        .ok_or(Error::NotFound)?;
    let mut request =
        ActionRequest::new(action_key, idempotency_key, available.subject_version).for_entry(entry);
    if let Some(payload) = payload {
        request.payload = serde_json::from_str(payload).map_err(|_| Error::Invalid)?;
    }
    Ok(request)
}
fn print_result(label: &str, result: Result<ActionResult>) {
    match result {
        Ok(result) => println!(
            "{label}: request={} action={} replayed={} outcome={} work={:?} result_ref={:?} output={}",
            result.request_id,
            result.action_key,
            result.replayed,
            result.outcome,
            result.work_item_id,
            result.result_ref,
            result.output
        ),
        Err(error) => println!("{label}: refused {error:?}"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("BILIBI_DISPOSABLE_WORKFLOW_DEMO").as_deref() != Ok("yes") {
        return Err(Error::Invalid);
    }
    bilibi_base::link();
    bilibi_core::link();
    let db = pool(&std::env::var("DATABASE_URL").map_err(|_| Error::Invalid)?)?;
    let modules = bilibi_module_registry::discover()?;
    let migrations: Vec<_> = modules
        .iter()
        .flat_map(|m| m.migrations.iter().copied())
        .collect();
    bilibi_database::migrate(&db, &migrations).await?;
    let mut services = Services::default();
    services.provide("bootstrap.database", Arc::new(db.clone()))?;
    services.install(&modules)?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("show");
    let user = args.get(1).and_then(|s| s.parse::<i64>().ok()).unwrap_or(4);
    let id = args.get(2).and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
    let client = db.get().await?;
    let scope = services
        .get::<Authentication>("base/tenancy.authentication")?
        .authenticate(&**client, 1, user, String::new())
        .await?;
    drop(client);
    let engine = services.get::<Engine>("base/workflow.engine")?;
    match command {
        "init" => {
            let client = db.get().await?;
            // Disposable specimen only: initialize needs the platform operator.
            client.execute("INSERT INTO platform_operator(singleton,tenant_id) VALUES(true,1) ON CONFLICT DO NOTHING",&[]).await?;
            drop(client);
            engine.initialize(&scope).await?;
            let client = db.get().await?;
            client.execute("INSERT INTO workflow_reference_records(id,tenant_id) SELECT n,1 FROM generate_series(1,20) n ON CONFLICT DO NOTHING",&[]).await?;
            client.execute("INSERT INTO workflow_reference_records(id,tenant_id) VALUES(100,2) ON CONFLICT DO NOTHING",&[]).await?;
            println!("Reference definitions/records initialized (insert only)");
        }
        "show" => {
            let client = db.get().await?;
            for row in client.query("SELECT id,status,version,action_count,facade_value FROM workflow_reference_records WHERE tenant_id=1 ORDER BY id",&[]).await? {println!("id={} status={} version={} actions={} facade={}",row.get::<_,i64>(0),row.get::<_,String>(1),row.get::<_,i64>(2),row.get::<_,i32>(3),row.get::<_,i32>(4));}
        }
        "history" => {
            let page = engine
                .history(&scope, "ReferenceRecord", id, None, 50)
                .await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&page.entries).map_err(|_| Error::Internal)?
            );
        }
        "adopt" => println!(
            "binding={}",
            engine.adopt(&scope, "ReferenceRecord", id).await?
        ),
        "available" => println!(
            "{}",
            serde_json::to_string(&engine.available(&scope, "reference.record", id).await?)
                .map_err(|_| Error::Internal)?
        ),
        "start" | "start-v1" | "start-changed" | "start-deny" | "drift" | "start-lifecycle"
        | "start-worker" | "start-human" => {
            // Keep the v1 drift proof separate from the default v2 replay key.
            let attempt = if matches!(command, "start-v1" | "drift") {
                1
            } else {
                2
            };
            let mut options = StartOptions::new(format!("record:{id}:attempt:{attempt}"));
            let process_key = match command {
                "start-lifecycle" => "reference.lifecycle",
                "start-worker" => "reference.worker",
                "start-human" => "reference.human",
                _ => "reference.review",
            };
            options.version = (command == "start-v1").then_some(1);
            options.correlation_key = Some(format!("reference-record:{id}"));
            if command == "start-changed" {
                options.input = json!({"carried_fact_ids":[42]});
            }
            if command == "start-deny" {
                options.idempotency_key = format!("record:{id}:denied");
                options.input = json!({"deny":true});
            }
            let engine = if command == "drift" {
                // Registry::build is the boot validator; the drifted process
                // replaces the installed v1 under the same key/version.
                let entries: Vec<(&'static str, &Contribution)> = inventory::iter::<Registration>
                    .into_iter()
                    .filter(|entry| entry.consumer == CONTRIBUTIONS)
                    .map(|entry| {
                        let payload = if entry.id == "reference.process.v1" {
                            &DRIFTED_V1
                        } else {
                            entry.get::<Contribution>()?
                        };
                        Ok((entry.provenance.module, payload))
                    })
                    .collect::<bilibi_module_registry::Result<_>>()?;
                let registry = Registry::build(&entries, Some(bilibi_authz::vocabulary()?))?;
                options.version = Some(1);
                Arc::new(Engine::new(
                    db.clone(),
                    services.get::<MutationBoundary>("base/database.mutations")?,
                    services.get::<Authentication>("base/tenancy.authentication")?,
                    registry,
                ))
            } else {
                engine.clone()
            };
            let (key, version, fingerprint) = {
                let definition = engine.registry().process(process_key, options.version)?;
                (
                    definition.key,
                    definition.version,
                    definition.fingerprint.clone(),
                )
            };
            println!("compiled {key} v{version} fingerprint={fingerprint}");
            match engine
                .start_run(&scope, process_key, "ReferenceRecord", id, options)
                .await
            {
                Ok(run) => print_view(&engine.get_run(&scope, run.id).await?),
                Err(error) => println!("start refused: {error:?}"),
            }
        }
        "complete"
        | "complete-stale"
        | "complete-executor-mismatch"
        | "complete-outer-fail"
        | "signal"
        | "pause"
        | "resume"
        | "supersede"
        | "waive"
        | "block"
        | "reconcile" => {
            let value = args.get(3).map(String::as_str);
            let command_value = match command {
                "signal" => RunCommand::Signal {
                    name: value.unwrap_or("review.approved").into(),
                    payload: args
                        .get(4)
                        .map(|v| serde_json::from_str(v))
                        .transpose()
                        .map_err(|_| Error::Invalid)?
                        .unwrap_or(json!({"approved":true})),
                    idempotency_key: args.get(5).cloned().unwrap_or("approval:1".into()),
                },
                "pause" => RunCommand::Pause {
                    reason: value.unwrap_or("Operator review").into(),
                },
                "resume" => RunCommand::Resume,
                "supersede" => RunCommand::Supersede {
                    reason: value.unwrap_or("Replaced by a new process").into(),
                },
                "reconcile" => RunCommand::Reconcile,
                "waive" => RunCommand::Waive {
                    step_key: value.unwrap_or("render").into(),
                    reason: args.get(4).cloned().unwrap_or("Operator waived".into()),
                    outcome: args.get(5).cloned().unwrap_or("waived".into()),
                },
                "block" => RunCommand::Block {
                    step_key: value.unwrap_or("notify").into(),
                    reason: args.get(4).cloned().unwrap_or("Operator blocked".into()),
                },
                _ => {
                    let view = engine.get_run(&scope, id).await?;
                    let item = view
                        .work_items
                        .iter()
                        .find(|i| Some(i.step_key.as_str()) == value)
                        .ok_or(Error::NotFound)?;
                    RunCommand::Complete {
                        step_key: item.step_key.clone(),
                        completion: Completion {
                            expected_version: if command == "complete-stale" {
                                item.version + 1
                            } else {
                                item.version
                            },
                            executor_key: if command == "complete-executor-mismatch" {
                                "wrong.executor".into()
                            } else {
                                item.executor_key.clone()
                            },
                            outcome: args.get(4).cloned().unwrap_or("completed".into()),
                            output: json!({"recorded_fact":"preserve across resume"}),
                            result_ref: Some(format!("reference:{}", item.step_key)),
                        },
                    }
                }
            };
            if command == "complete-outer-fail" {
                let view = engine.get_run(&scope, id).await?;
                let subject_id = view.run.subject_id;
                let mut client = db.get().await?;
                let boundary = services.get::<MutationBoundary>("base/database.mutations")?;
                let mut tx = boundary
                    .begin(
                        &mut client,
                        scope.mutation_context("reference.facade.updated"),
                    )
                    .await?;
                tx.mutate(move |sql|Box::pin(async move {
                    sql.execute("UPDATE workflow_reference_records SET facade_value=facade_value+1 WHERE tenant_id=1 AND id=$1",&[&subject_id]).await?;
                    Ok(((),vec![Change {kind:"reference.facade".into(),id:subject_id,old:json!({"increment":0}),new:json!({"increment":1})}]))
                })).await?;
                let view = engine
                    .update_run_in(&mut tx, &scope, id, command_value)
                    .await?;
                println!(
                    "completion inside owner transaction status={} (rolling back owner facade)",
                    view.run.status
                );
                tx.rollback().await?;
            } else {
                match engine.update_run(&scope, id, command_value).await {
                    Ok(view) => print_view(&view),
                    Err(error) => println!("command refused: {error:?}"),
                }
            }
        }
        "claim" | "claim-race" => {
            // A worker process: claims, prints its durable claim and exits.
            // Exiting before finishing is the crash the lease recovers from.
            let options = ClaimOptions {
                lease_seconds: args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2),
                definition_key: Some("reference.worker".into()),
                run_ids: Some(vec![id]),
                executor_keys: args.get(5).map(|key| vec![key.clone()]),
            };
            let worker = args.get(4).cloned().unwrap_or("worker-a".into());
            let claims = if command == "claim-race" {
                let (a, b) = tokio::join!(
                    engine.claim_work(&scope, "worker-a", options.clone()),
                    engine.claim_work(&scope, "worker-b", options)
                );
                vec![a, b]
            } else {
                vec![engine.claim_work(&scope, &worker, options).await]
            };
            for claimed in claims {
                match claimed {
                    Ok(Some(claimed)) => {
                        println!(
                            "claimed step={} attempt={} lease_expires_at={:?}",
                            claimed.work_item.step_key,
                            claimed.work_item.attempts,
                            claimed.work_item.lease_expires_at
                        );
                        println!(
                            "claim={}",
                            serde_json::to_string(&claimed.claim).map_err(|_| Error::Internal)?
                        );
                    }
                    Ok(None) => println!("nothing claimable"),
                    Err(error) => println!("claim refused: {error:?}"),
                }
            }
        }
        "heartbeat" | "complete-claimed" | "fail" | "block-claimed" => {
            // The record-ID position takes the claim JSON a `claim` printed.
            let claim: Claim = args
                .get(2)
                .and_then(|s| serde_json::from_str(s).ok())
                .ok_or(Error::Invalid)?;
            let value = args.get(3).cloned();
            let work = match command {
                "heartbeat" => WorkCommand::Heartbeat {
                    lease_seconds: value.and_then(|s| s.parse().ok()).unwrap_or(30),
                },
                "complete-claimed" => WorkCommand::Complete {
                    outcome: value.unwrap_or("completed".into()),
                    output: json!({"worker_fact":"saved once"}),
                    result_ref: None,
                },
                "fail" => WorkCommand::Fail {
                    error: "Source unavailable".into(),
                    retryable: value.as_deref() != Some("final"),
                    category: "transient".into(),
                    retry_at: None,
                },
                _ => WorkCommand::Block {
                    reason: value.unwrap_or("Source rejected the request".into()),
                    output: json!(null),
                    result_ref: None,
                },
            };
            match engine.update_work(&scope, &claim, work).await {
                Ok(WorkView { run, work_item }) => println!(
                    "run={} status={} work={} step={} status={} attempts={}/{} lease_expires_at={:?} outcome={:?} version={} error={:?}",
                    run.id, run.status, work_item.id, work_item.step_key, work_item.status, work_item.attempts, work_item.max_attempts, work_item.lease_expires_at, work_item.outcome, work_item.version, work_item.last_error
                ),
                Err(error) => println!("worker call refused: {error:?}"),
            }
        }
        "actions" => println!(
            "{}",
            serde_json::to_string_pretty(
                &engine
                    .available_actions(&scope, "reference.record", id)
                    .await?
            )
            .map_err(|_| Error::Internal)?
        ),
        "act" | "act-stale" | "act-work-stale" | "act-race" | "act-outer-fail" => {
            // Arguments: action key, idempotency key, optional JSON payload.
            let action_key = args
                .get(3)
                .map(String::as_str)
                .unwrap_or("reference.acknowledge");
            let key = args.get(4).map(String::as_str).unwrap_or("ack:1");
            let mut request =
                page_request(&engine, &scope, id, action_key, key, args.get(5)).await?;
            match command {
                "act-stale" => request.expected_subject_version += 1,
                "act-work-stale" => {
                    request.expected_work_version = request.expected_work_version.map(|v| v + 1)
                }
                _ => {}
            }
            println!(
                "page: subject_version={} run={:?} work={:?} work_version={:?}",
                request.expected_subject_version,
                request.process_run_id,
                request.work_item_id,
                request.expected_work_version
            );
            if command == "act-race" {
                // Two connections submit the same request from the same page.
                let (a, b) = tokio::join!(
                    engine.execute_action(&scope, "reference.record", id, request.clone()),
                    engine.execute_action(&scope, "reference.record", id, request)
                );
                print_result("first", a);
                print_result("second", b);
            } else if command == "act-outer-fail" {
                let mut client = db.get().await?;
                let boundary = services.get::<MutationBoundary>("base/database.mutations")?;
                let mut tx = boundary
                    .begin(
                        &mut client,
                        scope.mutation_context("reference.facade.updated"),
                    )
                    .await?;
                print_result(
                    "inside owner transaction",
                    engine
                        .execute_action_in(&mut tx, &scope, "reference.record", id, request)
                        .await,
                );
                tx.rollback().await?;
                println!("owner transaction rolled back");
            } else {
                print_result(
                    command,
                    engine
                        .execute_action(&scope, "reference.record", id, request)
                        .await,
                );
            }
        }
        "act-race-cross" => {
            // One idempotency key raced on two subjects of the same tenant.
            let other = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(id + 1);
            let key = args.get(4).map(String::as_str).unwrap_or("cross:1");
            let wait = Some("{\"handler_wait\":true}".to_string());
            let first = page_request(
                &engine,
                &scope,
                id,
                "reference.acknowledge",
                key,
                wait.as_ref(),
            )
            .await?;
            let second = page_request(
                &engine,
                &scope,
                other,
                "reference.acknowledge",
                key,
                wait.as_ref(),
            )
            .await?;
            let (a, b) = tokio::join!(
                engine.execute_action(&scope, "reference.record", id, first),
                engine.execute_action(&scope, "reference.record", other, second)
            );
            print_result(&format!("record {id}"), a);
            print_result(&format!("record {other}"), b);
        }
        "run" | "run-drift" => {
            let engine = if command == "run-drift" {
                let entries: Vec<(&'static str, &Contribution)> = inventory::iter::<Registration>
                    .into_iter()
                    .filter(|entry| entry.consumer == CONTRIBUTIONS)
                    .map(|entry| {
                        let payload = if entry.id == "reference.process.v1" {
                            &DRIFTED_V1
                        } else {
                            entry.get::<Contribution>()?
                        };
                        Ok((entry.provenance.module, payload))
                    })
                    .collect::<bilibi_module_registry::Result<_>>()?;
                Arc::new(Engine::new(
                    db.clone(),
                    services.get::<MutationBoundary>("base/database.mutations")?,
                    services.get::<Authentication>("base/tenancy.authentication")?,
                    Registry::build(&entries, Some(bilibi_authz::vocabulary()?))?,
                ))
            } else {
                engine.clone()
            };
            match engine.get_run(&scope, id).await {
                Ok(view) => println!(
                    "{}",
                    serde_json::to_string_pretty(&view).map_err(|_| Error::Internal)?
                ),
                Err(error) => println!("read refused: {error:?}"),
            }
        }
        "events" => {
            let page = engine.run_events(&scope, id, None, 50).await?;
            for event in page.entries {
                println!(
                    "seq={} type={} work={:?} payload={}",
                    event.sequence,
                    event.kind,
                    event.work_item_id,
                    event.payload.unwrap_or(json!(null))
                );
            }
            println!("next_cursor={:?}", page.next_cursor);
        }
        "initial" | "comment" => {
            let request = Request {
                comment: if command == "comment" {
                    Some("Review note".into())
                } else {
                    None
                },
                ..Request::default()
            };
            let fact = engine
                .record(
                    &scope,
                    "reference.record",
                    id,
                    if command == "initial" {
                        Operation::RecordInitial
                    } else {
                        Operation::RecordComment
                    },
                    request,
                )
                .await?;
            println!("history={} kind={}", fact.id, fact.kind);
        }
        "approve" | "guard-deny" | "action-fail" | "bad-persist" | "stale" | "outer-fail"
        | "cancel" => {
            let input = match command {
                "guard-deny" => json!({"guard_deny":true}),
                "action-fail" => json!({"action_fail":true}),
                "bad-persist" => json!({"bad_persist":true}),
                "cancel" => json!({"action_wait":true}),
                _ => json!({}),
            };
            let request = Request {
                expected_version: Some(if command == "stale" { 99 } else { 0 }),
                input,
                ..Request::default()
            };
            if command == "outer-fail" || command == "cancel" {
                let mut client = db.get().await?;
                let boundary = services.get::<MutationBoundary>("base/database.mutations")?;
                let mut tx = boundary
                    .begin(
                        &mut client,
                        scope.mutation_context("reference.facade.updated"),
                    )
                    .await?;
                tx.mutate(|sql|Box::pin(async move {
                    sql.execute("UPDATE workflow_reference_records SET facade_value=facade_value+1 WHERE tenant_id=1 AND id=$1",&[&id]).await?;
                    Ok(((),vec![Change {kind:"reference.facade".into(),id,old:json!({"increment":0}),new:json!({"increment":1})}]))
                })).await?;
                if command == "cancel" {
                    let result = tokio::time::timeout(
                        std::time::Duration::from_millis(200),
                        engine.transition_in(
                            &mut tx,
                            &scope,
                            "reference.record",
                            id,
                            "approved",
                            request,
                        ),
                    )
                    .await;
                    println!("transition timed out={}", result.is_err());
                } else {
                    let mut request = request;
                    request.input = json!({"action_fail":true});
                    println!(
                        "transition result={:?}",
                        engine
                            .transition_in(
                                &mut tx,
                                &scope,
                                "reference.record",
                                id,
                                "approved",
                                request
                            )
                            .await
                    );
                }
                println!("outer commit={:?}", tx.commit().await);
            } else {
                let fact = engine
                    .transition(&scope, "ReferenceRecord", id, "approved", request)
                    .await?;
                println!(
                    "history={} status={} version={}",
                    fact.id, fact.status, fact.version
                );
            }
        }
        _ => return Err(Error::Invalid),
    }
    Ok(())
}
