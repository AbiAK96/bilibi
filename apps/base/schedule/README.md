# Schedule definitions, review and occurrence claims

Schedule holds installation-global recurrence definitions, the operator's
decision about them, and the durable claim that hands exactly one Queue job to
each intended occurrence. Definitions, fingerprint review, suppression and the
cron/zone arithmetic are Q3; occurrence claiming, the Queue insertion, overlap
leases and run history are Q4 (this slice); the operator board is Q5. Work
itself runs on the Queue, never inline.

Owners contribute a `Definition` to `base/schedule.definitions` with
`Contribution::new`; the contribution id is the durable key
(`^[a-z0-9][a-z0-9._/-]{0,254}$`) and the provenance module is the owner. A
definition names a five-field cron, an IANA zone, a task name, a Queue `worker`
id, bounded arguments, an overlap policy and the one misfire policy
(`Coalesce`). The worker must be a `base/queue.workers` entry owned by the same
module and must accept the arguments; boot refuses unknown zones, macros
(`@daily`), seconds/year fields, `L`/`W`/`#`/`?`, expressions that can never
match a date (`0 0 31 2 *`), unbounded arguments, the reserved
`__bilibi_schedule__` argument and an owner route that is not an internal path.
As in Bilimbi, a restricted day-of-month and a restricted weekday must both match.

## Scheduled workers: use `worker::validate` and `Schedule::run`

The claim enqueues the definition's arguments plus bounded claim metadata under
`RESERVED_ARGUMENT`. A scheduled worker therefore must:

* validate through `bilibi_schedule::worker::validate(args, mine)`, so the
  owner's validator sees only its own arguments and the metadata is kept. Boot
  refuses a definition whose worker drops that metadata, so a hand-rolled
  validator cannot get as far as a refused claim;
* bind its handler (`base/queue.handlers`) to the capability
  `bilibi_schedule::EXECUTE`, the one the `scheduler` system principal is
  declared with;
* run its `Handler::perform` body through `Schedule::run(execution, args, |args|
  ...)`. Before the owner's code starts, under the per-key lock, the definition
  must still exist with the same fingerprint, be enabled and unsuppressed, and
  the durable occurrence must be unfinished and bound to this job; otherwise the
  occurrence is failed and the job cancelled with `schedule_removed`,
  `schedule_changed`, `schedule_unreviewed`, `schedule_disabled`,
  `schedule_suppressed` or `invalid_schedule_occurrence`. Pausing, disabling or
  materially changing a definition therefore cancels work that has not begun.
  Each attempt is a `base_schedule_runs` row; a job without metadata is
  cancelled with `missing_schedule_metadata`.

The owner still makes its business effect idempotent: Queue is at least once,
and the occurrence claim prevents duplicate enqueue, not duplicate effects after
a worker failure.

## Who runs the work

A recurrence has no user behind it. The scheduler enqueues as the named system
principal `scheduler` (`PRINCIPAL`) in the platform operator's tenant
(`platform_operator`); without an operator nothing is claimed. Declaration
grants nothing: an operator must grant the principal `EXECUTE` in that tenant
(`Grants::set_system(admin, "scheduler", EXECUTE, Some(true))`, capability
`admin.authz.system-principal.grant`). Queue re-proves the principal, its
declaration and that grant on every attempt (`Queue::enqueue_system`,
`Execution::identity`); an ungranted principal's jobs cancel with
`actor_refused`. Run-now (`Schedule::run_now`) queues a `manual` occurrence as
the operator who asked, after `authorize_operator(EXECUTE)`, so that job runs as
that user and its run row names them.

## Claims, coalescing and overlap

`Scheduler` polls every 15 s in each app process (`bilibi-web run` starts it
beside the Queue executor; `Scheduler::poll(now)` takes the clock so any moment
can be replayed). Each poll first settles unfinished occurrences whose job
Queue already reports terminal (`completed` is `succeeded`, cancelled,
discarded or missing is `failed`, and the lease is released), then for every
definition computes `previous(now)` and claims it when it is newer than the
key's watermark (the latest intended instant ever claimed). Downtime
**coalesces**: only the latest missed occurrence is considered, the instants in
between are never claimed. Both passes of a DST fold are real occurrences; a
gap's missing minute is dropped.

`Schedule::claim` is one audited transaction under `lock(key)`: eligibility,
reconciliation of that key, the `(key, intended_at, trigger)` uniqueness check,
the forbid-overlap check, the Queue job (`base/queue.job`) and the occurrence
row (`base/schedule.occurrence`) commit or roll back together, so two app
processes polling the same instant produce exactly one occurrence and one job;
the loser sees `AlreadyClaimed`. `Overlap::Forbid` sets `overlap_key` on the
unfinished occurrence (a partial unique index: one active lease per key); a
later occurrence is refused with `Overlap` and recorded once as a `skipped` run
until the earlier job succeeds, is cancelled or exhausts its retries. Refusals
write nothing.

`base_schedule_runs` is best-effort operational evidence (running, succeeded,
failed, skipped; trigger and who asked for a manual run); recording failure is
logged and never changes the owner's outcome. `Schedule::runs` reads it
redacted with `admin.system.schedule.view`. Retention pruning and the board
are Q5.

## Review and suppression

Every definition is `Unreviewed` until an operator reviews its **fingerprint**
(`Registered::fingerprint`): SHA-256 over key, name, expression, zone, owner,
task name, worker, canonicalised arguments, overlap and misfire. Changing any of
those in code makes a stored review stale, so a redeploy can never silently run
changed work; the owner route is display-only and not part of it. Review state
is `Unreviewed`, `Enabled` or `Disabled` for the current fingerprint.
Suppression is separate: a row in `base_schedule_suppressions` means paused and
deleting it means resumed. Rows are keyed by text, not foreign keys, so a removed
or renamed definition leaves its row as orphan evidence and a new key can never
inherit a decision.

`Schedule::review`, `suppress` and `resume` take the sealed `Scope`, require the
persistent operator identity plus `admin.system.schedule.manage`
(`authorize_operator`), and write the state and its Audit evidence in one
`MutationTransaction` under a per-key advisory lock (`schedule::lock`). A
repeated `suppress`/`resume` changes nothing and records nothing. `tasks` needs
`admin.system.schedule.view` and returns redacted facts (no arguments).
`admin.system.schedule.execute` is declared for run-now (Q4/Q5).

`Schedule::eligibility` is what a claim or a started job must ask, inside the
transaction that holds `lock`: `Eligible` only for an enabled, unsuppressed
current fingerprint. Any database error is an `Err`, never `Eligible`.

## Time

`cron` knows only civil minutes. `recurrence` is the only place a zone matters,
and every function takes the instant as an argument:

* **Fold** (clocks repeat): both instants are real. `previous(now)` is the latest
  that has happened, so a poll before the repeat gets the first and a poll after
  it gets the second; `next(now)` is the earliest still strictly ahead.
* **Gap** (clocks skip): the occurrence does not exist and is dropped, not
  shifted. `next` resumes from the first wall time after the gap; `previous`
  searches before it.
* `previous` is Bilimbi's exact rule. `next` is display only: Bilimbi's
  wall-clock search skips to the following day while the first pass of a fold is
  running, so `next` here also considers the repeat that is still ahead.

Replay any moment with `cargo run -p bilibi-schedule --example schedule-walkthrough -- dst [key instant]`
(no database). The other modes use a disposable database
(`BILIBI_DISPOSABLE_SCHEDULE_DEMO=yes`, `DATABASE_URL`, after `bilibi-web migrate`
and `seed`); they link a private worker, handler and five definitions that
production does not have:

```sh
export BILIBI_DISPOSABLE_SCHEDULE_DEMO=yes
cargo run -p bilibi-schedule --example schedule-walkthrough -- review    # Q3: review, suppression, audit
cargo run -p bilibi-schedule --example schedule-walkthrough -- prepare   # operator tenant, enable, grant scheduler
cargo run -p bilibi-schedule --example schedule-walkthrough -- coalesce  # one claim after five missed hours
cargo run -p bilibi-schedule --example schedule-walkthrough -- race 2026-06-03T10:00:00Z   # two processes, one row
cargo run -p bilibi-schedule --example schedule-walkthrough -- overlap   # lease refuses, skipped run, cancel releases
cargo run -p bilibi-schedule --example schedule-walkthrough -- execute 2026-06-04T12:00:10Z # scheduler/user/stale review
cargo run -p bilibi-schedule --example schedule-walkthrough -- worker    # live, real clock; run it in two terminals
cargo run -p bilibi-schedule --example schedule-walkthrough -- inspect
```

`worker` claims the every-minute definitions and executes them; a second
`worker` process (set `SCHEDULE_DEMO_KEEP=1` so it does not reset the first)
shows that each minute is claimed by exactly one of them, and the 75-second
task's next minute is refused for overlap and recorded as skipped while it runs.
