//! The scheduled history prune (Bilimbi `Perf.RetentionWorker`): daily, once an
//! operator enables it on the Schedule board. It is a Perf worker, not part of
//! the recorder, so retention still runs if recording is switched off.
use crate::recorder::Recorder;
use bilibi_database::{Error, Result};
use bilibi_module_registry::{Contribution, Provenance};
use bilibi_queue::{
    execution::{Execution, Handler, HandlerBinding, HandlerFuture, Outcome, HANDLERS},
    Worker, WORKERS,
};
use bilibi_schedule::{worker, Definition, Misfire, Overlap, Schedule, DEFINITIONS, EXECUTE};
use serde_json::{json, Value};
use std::sync::Arc;

const WORKER: &str = "base/perf-retention";
macro_rules! provenance {
    () => {
        Provenance {
            module: "base/perf",
            source: file!(),
            line: line!(),
        }
    };
}

fn no_arguments(args: Value) -> Result<Value> {
    match args.as_object() {
        Some(object) if object.is_empty() => Ok(args),
        _ => Err(Error::Invalid),
    }
}
inventory::submit! { Contribution::new(WORKERS, WORKER, provenance!(), &Worker { max_attempts: 5, validate: |args| worker::validate(args, no_arguments) }) }
inventory::submit! { Contribution::new(HANDLERS, WORKER, provenance!(), &HandlerBinding { service: "base/perf.retention", capability: EXECUTE }) }
inventory::submit! { Contribution::new(DEFINITIONS, "base/perf.retention", provenance!(), &Definition {
    name: "Prune performance history", expression: "17 3 * * *", timezone: "Etc/UTC",
    task_name: "Base Perf retention", worker: WORKER, args: || json!({}),
    overlap: Overlap::Forbid, misfire: Misfire::Coalesce, owner_route: None,
}) }

pub(crate) struct Retention {
    pub schedule: Arc<Schedule>,
    pub recorder: Arc<Recorder>,
}
impl Handler for Retention {
    fn perform<'a>(&'a self, execution: &'a Execution, args: Value) -> HandlerFuture<'a> {
        Box::pin(self.schedule.run(execution, args, |_| async {
            match self.recorder.prune().await {
                Ok(_) => Outcome::Completed,
                Err(_) => Outcome::Retry("perf_store_unavailable"),
            }
        }))
    }
}
