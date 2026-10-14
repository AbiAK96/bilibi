# Private artifacts (A1)

Use `base/artifacts.store` and `Artifacts::for_owner(scope, company, &ConcreteOwner)`
for sensitive uploaded/generated documents. Implement `Owner` in the business
module and declare a dependency on `base/artifacts`. Select the concrete adapter
in server code; never select one by a request parameter. Its stable `owner_id`
and Rust type name are persisted together. Renaming either needs an explicit
owner-reviewed data migration. There is no Base query of Company tables.

Every callback must prove a live company in the tenant, current actor authority,
and the live subject for that operation. `Purge` has no reference and requires
company maintenance access. Base reauthenticates the sealed account on every
operation and refuses changed party facets or a different bound company.
`Operation::Retain` is reserved for A2. The owner validates upload MIME/content
and scanning policy; `content_type` is descriptive metadata only.

```rust,ignore
let documents = artifacts.for_owner(&scope, company_id, &DocumentOwner);
let metadata = documents.put(&mut client,
    Reference { subject: record_id, kind: "evidence".into() },
    &bytes, "application/pdf").await?;
let document = documents.read(&mut client, &metadata.id).await?;
documents.delete(&mut client, &metadata.id).await?;
let report = documents.recover(&mut client, 100).await?;
```

These methods require a plain PostgreSQL `Client`, outside any business
transaction. Reservations and tombstones must commit before filesystem work.
No file path, filename or content enters audit payloads. Metadata returned to an
owner contains no storage path. Reads hold a row lock, recheck permission and
expiry, validate 0600 regular-file mode, byte count and SHA-256, and commit audit
evidence before bytes escape. Owners' authenticated download routes must call
`read` every time and return attachments with `Cache-Control: private, no-store`
and `X-Content-Type-Options: nosniff`. A1 adds no public download route or screen.

Operator Settings owns these installation values, gated by
`admin.system.artifacts.manage` and explicit platform operator identity:

- `artifacts.storage_root`: existing absolute private directory, initially empty.
- `artifacts.retention_days`: required positive integer, initially unset.
- `artifacts.max_bytes`: initially 10 MiB, maximum 512 MiB.

Provision the directory with 0700 mode. Files use exclusive random UUID names,
0600 mode and `O_NOFOLLOW`. Root components cannot be symlinks or named
public/static/assets. The root and parents must be controlled by trusted OS
operators; never expose them through a proxy/static server. Root changes affect
new artifacts only. Keep old roots accessible until their records are purged.
Back up metadata and the durable private volume together.

Publication commits an audited reservation, locks it, writes/syncs bytes and the
directory, rechecks authority, then publishes. If writing, permission, SQL or
audit fails, the inaccessible reservation remains tracked. An ambiguous commit
may have published: never blindly remove bytes after a database error.

Deletion commits a tombstone first. Physical cleanup then holds the row lock,
unlinks/syncs the directory and marks the tombstone purged in an audited commit.
A failed unlink leaves a retryable tombstone. If the purge commit fails after
unlink, retry accepts an absent file and commits the evidence. Purged metadata
remains; database guards forbid hard deletion, authority substitution and
lifecycle reversal.

`recover` authorizes maintenance before selecting an ordered bounded batch for
this tenant/company/concrete owner, then reauthorizes deletion per candidate.
It reconciles pending tombstones and unpublished reservations older than one
hour. Rechecking under the row lock protects a writer that published after
candidate selection. Filesystem operations are synchronous and bounded while
that lock is held: cancellation cannot leave a background writer creating bytes
after recovery releases the reservation. Tokio multithread runtimes use `block_in_place` to let other tasks run; use
a dedicated worker/runtime thread when using a single-thread runtime. Recovery reports individual errors and keeps failed
rows eligible; callers own retry pacing and operational alerts. A2 will add
records-grade retention, expiry purge, hold/retry evidence and scheduling policy.

No unit tests. `examples/artifacts-walkthrough.rs` is an explicit disposable-DB
walkthrough of real facade calls and injected DB/filesystem outages, with output
for manual inspection. It does not run assertions or use a test harness.

Run the walkthrough after migrating and seeding an isolated database:

```sh
export DATABASE_URL=postgres://bilibi@127.0.0.1:55481/bilibi_a1
export BILIBI_DISPOSABLE_ARTIFACTS_DEMO=yes
export BILIBI_ARTIFACTS_DEMO_ROOT=/absolute/private/task-directory
cargo run -p bilibi-artifacts --example artifacts-walkthrough
cargo run -p bilibi-artifacts --example artifacts-walkthrough -- recover
```

The first command leaves durable failed reservations/tombstones and a ready
control document. The second starts a fresh process, reconciles the failures
(including a successful unlink whose audit commit failed), and preserves the
control. Repeat `recover` to see an empty successful batch. Fixture clock and
failure triggers are confined to the explicitly opted-in disposable database.
