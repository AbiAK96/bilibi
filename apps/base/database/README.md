# Database write and seed contracts

Business owners obtain `MutationBoundary` from the `base/database.mutations`
registry service. Start one transaction with trusted `Context`, and pass its
mutable `MutationTransaction` to every nested owner. Use `mutate` for writes and
return all affected records as `Change` values, including bulk writes. Owner SQL,
record capture, action capture and nested notifications commit together. A write,
capture or cancelled future poisons the transaction, so catching its error cannot
permit commit. Dropping the transaction rolls back. `sql()` exposes locking and
query operations; it is an explicit escape hatch, not automatic SQL interception.
Every business write using it must still call `record`. Sessions and migration or
seed ledgers are lifecycle writes, rather than business mutations.

The authoritative API is [src/transaction.rs](src/transaction.rs). Audit binds a
`Capture` implementation once through `MutationBoundary::bind_capture` during
service installation. Its methods receive the same PostgreSQL transaction and
must propagate failure. The transitional `DemoCapture` persists original
`audit_mutations` plus attribution JSON and a `bilibi_action_commits` row. That row
means a successful outer commit, not an attempted/refused action log. P5 owns the
full Audit implementation, redaction, attempted/refused action semantics and
subsequent Audit schema. `begin_with` also supports explicit deployment adapters.

`Context` carries tenant, actor `{kind,id}`, optional impersonator, action and one
trace ID shared by all nested changes. Database validates its shape; it does not
prove authorization. F3's session/worker adapters supply trusted attribution.
System and agent principals do not borrow a human ID; the demo's legacy numeric
`actor_id` is null for those kinds, with their identity preserved in `context`.
Historical mutation rows retain their original actor and have no context backfill.

Register infallible, synchronous notification/cache callbacks with `after_commit`.
They run in registration order after PostgreSQL accepts commit and never on
rollback, drop, capture failure or commit failure. NotificationBoundary registers
its own bell callback; business owners do not publish manually. A callback must
not panic or perform durable work: process failure can lose fan-out, and durable
work belongs in a transactional Queue/outbox. Share the outer boundary rather
than opening or committing an inner transaction.

# Schema ownership

`001-base`, `002-user` and `003-company` and their SQL files are frozen historical
demo migrations. Do not split, rename, move their registration, or replay their
DDL on databases where their versions were applied. `004-database-boundary` is
an additive bridge and Database ledger migration. Future tenancy, session,
authorization and audit DDL is contributed by those owning modules through
`Module::migrations`; Database only supplies orchestration. Use globally unique
module-prefixed versions, such as `base/settings/001-values`. The host runs
migrations in dependency order under a transaction advisory lock. A failed batch
rolls back both DDL and migration ledger rows.

# Production seeds

`cargo run -p bilibi-web -- seed-production` is explicit and separate from the
existing `seed` demo fixture. Application startup performs neither. There are no
production reference-data contributions in the demo yet, so running the command
currently applies none.

Contribute a typed `Seed` to the registry's `SEEDS` catalog with Production mode,
an immutable version, and either trusted SQL or a service operation. The runner
in [src/seeds.rs](src/seeds.rs) executes module dependencies first, then contribution
IDs within each module. Publish separate dependent modules when order is needed
between owners; within a module choose ordered IDs. Demo entries are excluded.
For a service seed, register `SeedService(Arc<dyn SeedExecutor>)` at the declared
service ID. The executor receives the runner's transaction and must use it for
all seed data. No extra checkout, transaction control or external IO is allowed.
Treat SQL/service operations as trusted deployment code, never client input.

The runner serializes with other seeds and migrations and commits the selected
batch's data together with `bilibi_production_seeds`. Failure leaves neither data
nor successful ledger entries from that batch; interruption can safely retry.
Repeated versions skip execution, preserving operator edits. Reusing an applied
version with a changed module, contribution ID, SQL or service/operation is refused.
Service implementations themselves must remain stable under an applied version;
publish a new version/operation when behavior changes. The definition ledger
stores SQL text, so put secrets behind service parameters rather than SQL literals.

# Runtime evidence

F2 was demonstrated with external runtime fixture executables and a disposable
PostgreSQL database, without unit tests. Evidence lives in Firstmate's task data
folder `data/bilibi-f2/`: legacy upgrade without replay, repeat demo seed preserving
an edit, shared Company/notification trace and bell fan-out, explicit rollback,
drop, swallowed capture failure, action-capture failure, cancelled owner/capture,
deferred commit failure, system/impersonator attribution, failed seed rollback and
retry, repeat seed skipping, definition drift refusal, SQL/service dependency
ordering, concurrent seed serialization and exclusion of Demo contributions.
