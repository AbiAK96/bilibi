# Workflow status kernel

Workflow owns status configuration, edges, subject bindings, append-only
history, and immutable versioned process runs with their lifecycle. Its facade accepts a sealed
`bilibi_tenancy::Scope`, a subject kind (public key or alias) and the owner's
record ID. It never queries an owner's table: the owner proves identity, tenant
and current status through its `SubjectAdapter`. Worker leases, human-action
replay and durable transition delivery are later slices.

## Contributions

An owner declares `base/workflow` as a module dependency and submits immutable
`bilibi_workflow::Contribution` values to the `CONTRIBUTIONS` consumer through
`bilibi_module_registry::Contribution::new` with source provenance:

- `Subject { key, aliases, adapter }`: one per owned record type.
- `Guard { key, aliases, adapter }` and `Action { key, aliases, adapter }`:
  owner hooks, referenced by key or alias from an edge.
- `Flow(Flow { code, label, subject, active, statuses, edges })`: default
  statuses and edges. Each edge may name a capability, guard and action.

Boot rejects a contributor that does not depend on `base/workflow`, invalid or
duplicate keys (lowercase dotted segments, at most 255 bytes), duplicate aliases,
a flow whose subject belongs to another module, two flows on one subject, edges
whose statuses are missing, whose `settings`/`metadata` are not JSON objects,
whose capability is not in the Authz vocabulary, or whose guard/action is not
owned by the same module. Contributions perform no I/O.

## Versioned processes

`Process { key, version, subject, adapter, steps }` contributes a durable run
graph for one registered subject type. Versions are positive integers, step
keys are unique, dependencies must name a step in the same graph, and cycles
are rejected at registry build time. Each process key keeps one owner and
subject across its installed versions.

The registry compiles every contribution into an immutable definition and a
SHA-256 fingerprint over its key, version and ordered step contracts. Step
field order is fixed, object keys in input and metadata are sorted recursively,
and array order is preserved. Floating-point values are rejected so a
serializer change cannot silently alter the fingerprint. Change the version
when changing any step contract, and retain old versions while their runs need
them.

The first authorized start inserts the key/version fingerprint into
`base_workflow_process_versions`; a database trigger refuses updates and
deletes. `Engine::start_run` and `start_run_in` ask both the subject adapter
and process adapter to authorize `StartRun` inside the audited transaction.
They namespace the caller's idempotency key by tenant and definition, create
the run, materialize its complete graph and append its initial events
atomically. Repeating a start returns the same run and work-item IDs when the
subject, version, fingerprint, input, priority and correlation key match; a
changed request conflicts.

`Engine::get_run` and `run_events` re-authenticate the scope, prove the live
subject, ask both adapters to authorize `ReadRun`, then check the saved
fingerprint and every persisted step and dependency before returning anything.
Reads take no row locks and graph reads are bounded by the installed definition
size. A changed contribution under a saved version conflicts; a malformed
saved graph is refused.

## Run lifecycle

`Engine::update_run(scope, run_id, command)` applies one `RunCommand`;
`update_run_in(&mut MutationTransaction, ...)` commits it with the owner's
other business writes, and a refusal poisons that whole transaction. Each
command re-authenticates the scope, locks the subject, then the run, then its
work items, and asks both adapters to authorize its own operation
(`ReconcileRun`, `CompleteWork`, `SignalRun`, `PauseRun`, `ResumeRun`,
`SupersedeRun`). `ReadRun` never grants any of them.

- `Complete { step_key, completion }` finishes an `available`, due item of a
  running run. `completion.expected_version` and `executor_key` must match the
  item, so a stale or misrouted completion conflicts. The outcome, output and
  `result_ref` are saved once; a completed item never changes again.
- A dependency accepts the outcomes it lists (default `completed`). An `All`
  step needs every dependency accepted; an `Any` step needs one. When no
  acceptable outcome can still arrive, the step is blocked with an event, and
  that block propagates downstream.
- `Signal { name, payload, idempotency_key }` marks every unfinished item that
  requires `name`. Repeating the same key and payload is a no-op; the same key
  with another signal or payload conflicts. Signals need a running run.
- A released step starts its timer at the later of the run's `available_at`
  and now, plus `delay_seconds`, and becomes `available` once due. `Reconcile`
  releases due timers; no background sweep exists yet.
- `Pause { reason }` and `Resume` toggle a running run; repeating either is a
  no-op. A paused run refuses completion and signals, and resuming re-settles
  the graph from its saved facts.
- `Supersede { reason }` blocks every unfinished item with cause
  `process_superseded`, keeps terminal work as recorded, and finishes the run.
  A live worker lease refuses it; a finished run returns unchanged.

Every write appends to the run's event log with a contiguous per-run
`sequence`, and the run finishes as `failed`, `blocked` or `completed` (in
that precedence) once every item is terminal.

`Engine::initialize` is the explicit insert-only seed for flows, statuses and
edges, run by a trusted admin or seed adapter holding
`admin.workflow.definition.manage`. Existing operator configuration always
wins, including inactive flags; boot never calls it and a changed contribution
never overwrites rows.

## Owner contract

All callbacks receive the kernel's `tokio_postgres::Transaction`. They must not
open or commit another transaction and must perform database effects only.

- `SubjectAdapter::load(tx, scope, id, Load::Read | Load::Lock)` scopes the
  query by tenant and, for `Lock`, locks and rereads the row with `FOR UPDATE`.
  It returns a `Subject` holding only proven id, tenant, status, version and
  plain JSON facts.
- `SubjectAdapter::authorize(tx, scope, subject, operation)` enforces owner
  policy for `Read`, `Transition`, `RecordInitial`, `RecordComment` and `Adopt`,
  even on edges without a capability.
- `SubjectAdapter::persist(tx, scope, subject, to, request)` writes the proven
  status from the locked status, increments the version and returns every
  change for audit capture. The kernel rereads the row and refuses a persist
  that left a different status or an unchanged version.
- `Guard::check` runs before persist; `Action::execute` runs after it with the
  new status. Any error or cancellation rolls back status, hook effects,
  binding, history and the audit facts together.

Use `Engine::transition(scope, kind, id, to, request)` for a standalone
transition, or `transition_in(&mut MutationTransaction, ...)` when the owner's
other business writes must commit with the transition. The engine runs inside
the owner's audited `mutate`, so a guard denial after an earlier owner write
poisons the whole transaction and the outer commit fails. The mutation context
must carry the same tenant and user as the scope; the scope is re-authenticated
inside the transaction. Lock order is owner row, then flow and status
definitions (`FOR SHARE`), then the subject binding (`FOR UPDATE`).

`Request` accepts `expected_version`, `comment` (≤ 16,000 bytes),
`comment_tag` (≤ 255 bytes), JSON arrays `assignees`/`attachments`, and JSON
objects `metadata`/`input`, at most 65,536 encoded bytes in total. `metadata`
reserves `_workflow` for sealed provenance. A mismatched `expected_version`
conflicts before any hook runs.

`available(scope, kind, id)` lists active edges between active statuses whose
capability the actor holds; guards run only during execution, so availability is
not authorization. `record(scope, kind, id, Operation::RecordInitial, request)`
refuses a subject that already has history; `RecordComment` appends a
same-status fact and requires a non-empty comment. `adopt(scope, kind, id)` locks
the subject under the owner's `Adopt` policy and binds it to its flow, inactive
flows included, without rewriting history. Rebinding a subject to another tenant
or owner conflicts.

History rows are append-only (a trigger refuses UPDATE and DELETE). A transition
fact's `tat` is the whole seconds since the subject's latest earlier fact that is
not a `record_comment`; it is null on a first transition and on recorded
facts. `history(scope, kind, id, after, limit)` returns a keyset page ordered by
id with `limit` in 1..=500 and `next_cursor` when more rows exist.

## Running proof (no unit tests)

Use an explicitly disposable PostgreSQL database, never the normal development
database. Migrate and seed the app fixture, then run the private owner example.
It links a reference subject, guard, action and flow that production never
contributes.

```sh
export DATABASE_URL='postgres://.../disposable'
export BILIBI_DISPOSABLE_WORKFLOW_DEMO=yes
cargo run -p bilibi-web -- migrate
cargo run -p bilibi-web -- seed
cargo run -p bilibi-workflow --example owner -- init
cargo run -p bilibi-workflow --example owner -- available 4 1
cargo run -p bilibi-workflow --example owner -- approve 4 1
cargo run -p bilibi-workflow --example owner -- history 4 1
```

To demonstrate versioned starts and reads in the same fresh disposable
database, run the process commands in order:

```sh
cargo run -p bilibi-workflow --example owner -- init
cargo run -p bilibi-workflow --example owner -- start-v1 4 1
cargo run -p bilibi-workflow --example owner -- start-v1 4 1
cargo run -p bilibi-workflow --example owner -- drift 4 1
cargo run -p bilibi-workflow --example owner -- run-drift 4 1
cargo run -p bilibi-workflow --example owner -- start 4 1
cargo run -p bilibi-workflow --example owner -- start-changed 4 1
cargo run -p bilibi-workflow --example owner -- run 4 3
cargo run -p bilibi-workflow --example owner -- events 4 3
```

The first repeated v1 start returns the same run and work IDs. `drift` and
`run-drift` refuse the changed graph under saved v1, and `start-changed`
refuses a changed input under the v2 idempotency key. A fresh database assigns
the v1 and v2 run IDs `1` and `3` in this sequence because the replay consumes
an unused database sequence value; use IDs printed by `start` if other runs
already exist.

To demonstrate the run lifecycle, start the six-step `reference.lifecycle`
graph and drive it (on a fresh database the run ID is `1`; use the ID `start`
prints otherwise):

```sh
cargo run -p bilibi-workflow --example owner -- start-lifecycle 4 1
cargo run -p bilibi-workflow --example owner -- complete 4 1 first accepted
cargo run -p bilibi-workflow --example owner -- pause 4 1 "Operator review"
cargo run -p bilibi-workflow --example owner -- complete 4 1 second
cargo run -p bilibi-workflow --example owner -- resume 4 1
cargo run -p bilibi-workflow --example owner -- complete 4 1 second
cargo run -p bilibi-workflow --example owner -- complete 4 1 any
cargo run -p bilibi-workflow --example owner -- complete 4 1 all
cargo run -p bilibi-workflow --example owner -- signal 4 1 review.approved
cargo run -p bilibi-workflow --example owner -- complete 4 1 signal
cargo run -p bilibi-workflow --example owner -- reconcile 4 1
cargo run -p bilibi-workflow --example owner -- events 4 1
```

Completing `first` releases `any` while `all` waits for `second`. The paused
run refuses `second`, and after `resume` the completed `first` keeps its
outcome and output. `signal` releases the approval step; repeating it is a
no-op and a different payload under the same key (`signal 4 1 review.approved
'{"approved":false}' approval:1`) conflicts. Completing `signal` starts the
two-second timer on `delayed`; `reconcile` releases it once due, after which
`complete 4 1 delayed` finishes the run. `complete-stale` and
`complete-executor-mismatch` conflict. On a second record, `complete 4 <run>
first rejected` blocks `all`, `signal` and `delayed`; `complete-outer-fail
4 <run> second` completes inside an owner transaction that rolls back, leaving
`second` available; `supersede 4 <run>` blocks the remaining work.

Arguments are `command [user-id] [record-id]`; user 4 is the seeded admin.
For `run`, `run-drift`, `events` and the lifecycle commands, the record-ID
position takes a process run ID. Lifecycle commands then take a step key,
signal name or reason; `complete` also takes an optional outcome.
`init` first marks tenant 1 as the platform operator in that disposable
database, because `Engine::initialize` requires the platform capability
`admin.workflow.definition.manage`; production operator bootstrap is K17.
Other commands: `show`, `initial`, `comment`, `adopt`, `guard-deny`,
`action-fail`, `bad-persist` (persist wrote the wrong status), `stale`
(expected version mismatch), `outer-fail` (owner facade write, then a failing
transition in the same transaction; the outer commit is refused and the facade
write rolls back) and `cancel` (the transition future is dropped mid-action and
the outer commit is refused). Repeat `show` and `history` after each refusal to
confirm nothing was written. Running `init` again changes nothing.
