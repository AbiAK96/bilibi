# Queue insertion (Q1)

Queue stores every job in PostgreSQL. Owners contribute a `Worker` through the
`base/queue.workers` typed catalog, using `Contribution::new` and source provenance.
Boot rejects invalid/duplicate worker IDs and attempts outside 1–100. The worker's
validator must reject unknown keys and enforce its own business types; it can
normalize values. Queue applies the transport bounds both before and after that
validator: object root, depth 8, 100 members per container, 128-byte keys,
16,384-byte strings, and 65,536 bytes encoded JSON. Only the default queue is
supported. Callers cannot choose attempts, transport metadata, or actor identity.

Use `Queue::enqueue_for` with the sealed `Scope` and the owner's existing
`MutationTransaction`. The audit context must match the tenant/user in that
scope. Queue inserts through `mutate`, so insertion or audit failure poisons the
transaction. Owner writes, job insertion, and audit evidence commit or roll back
together. A returned `JobRef` is provisional until the outer commit. Queue audit
rows exclude arguments; jobs should contain references, never secrets.

Actor user and optional Person/Organisation/Affiliation IDs are captured in
separate columns, together with worker owner and trace ID. Names, email, CSRF,
sessions and passwords are never copied. These are historical references, not
proofs of current authority. There are no foreign keys to Core identities: a job
survives deletion so execution can record an explicit refusal.

Q2 owns worker handlers, PostgreSQL claims, lease fencing, retries, cancellation,
lookup and current actor rehydration. The schema provides state, attempts,
availability, terminal timestamps, bounded failure codes and lease fields for
that work. Every execution must reprove current actor eligibility and authorize
business effects; at-least-once delivery requires owner idempotency. Q1 has no
executor, anonymous job path, uniqueness policy or named system-principal path.
Impersonation is refused until Tenancy supplies a sealed impersonator facet;
caller-supplied audit attribution cannot create execution authority.

## Running proof (no unit tests)

Use an explicitly disposable PostgreSQL database, never the normal development
database. Create it with your local PostgreSQL tools and set `DATABASE_URL` to
that database. Seed the existing app fixture, then run the owner walkthrough:

```sh
cargo run -p bilibi-web -- migrate
cargo run -p bilibi-web -- seed
BILIBI_DISPOSABLE_QUEUE_DEMO=yes cargo run -p bilibi-queue --example queue-walkthrough
BILIBI_DISPOSABLE_QUEUE_DEMO=yes cargo run -p bilibi-queue --example queue-walkthrough -- inspect
```

The walkthrough prints external visibility before commit, atomic owner/job/audit
rollback and commit, argument refusals, and mismatched scope/context refusal.
`inspect` runs in another process and prints persisted redacted job summaries.
The two contributed workers are linked only into the example; production has no
specimen worker. An empty production worker catalog is valid.
