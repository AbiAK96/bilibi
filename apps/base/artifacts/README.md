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

## Simple PDFs (A3)

Use `artifacts::pdf::render` for text and table documents rather than a
module-local PDF serializer, and store through `Owned::generate_pdf` or
`Owned::store_pdf` rather than calling `put` with unchecked bytes. HTML
printing, signed render URLs, passwords and queued rendering are later slices.

Implement `PdfOwner` (an `Owner` extension) on the same trusted adapter. `generate_pdf`
authorizes `Create` first, then calls `render_pdf(scope, company, reference,
data)`. The owner selects what the document contains from its own records and
the request `data`, formats dates and exact numbers into strings, and returns
complete PDF bytes. `generate_pdf` validates them, then `put` stores them as
`application/pdf` (reserve, publish, reauthorize, audit). Nothing is rendered
for an unauthorized actor, and refused output stores nothing.

```rust,ignore
impl PdfOwner for DocumentOwner {
    fn render_pdf<'a>(&'a self, scope: &'a Scope, company: OrganisationId,
        reference: &'a Reference, data: &'a Value) -> RenderFuture<'a> {
        Box::pin(async move {
            pdf::render(&Document {
                title: "Record summary".into(),
                metadata: Metadata { author: Some("Document owner".into()), ..Default::default() },
                blocks: vec![
                    Block::Text("Facts selected by the owning module.".into()),
                    Block::Table { headers: vec!["Record".into(), "Value".into()],
                        rows: vec![vec!["A".into(), "12.50".into()]] },
                    Block::PageBreak,
                ],
            })
        })
    }
}
let metadata = documents.generate_pdf(&mut client, reference, &json!({})).await?;
```

Layout is fixed and matches Bilimbi byte for byte: A4, built-in Courier 10 pt, 85
characters per line, 48 lines per page, equal-width columns (1–20) that wrap
without truncation, repeating table headers, rows that fit a page kept together
(larger rows continue across pages), blank lines preserved, tabs as spaces, CR/CRLF
as newlines. Explicit breaks never leave an empty trailing page. At most 2000
pages per document; split larger reports. Body text is printable ASCII plus Latin-1
U+00A0–U+00FF; anything else is `Error::UnsupportedText`, never substituted.
Metadata (title, `author`, `subject`, `keywords`) supports Unicode. Invalid shapes
return `Error::InvalidDocument`; errors never echo contents. Output is
deterministic: no timestamps, IDs or environment data, and content operands are
escaped so text cannot inject PDF syntax.

`pdf::validate` runs on every PDF before storage, whoever made it: `%PDF-` header,
`%%EOF` trailer and a `startxref` offset landing on an xref table or indirect
object. It is not a complete validator; the producing renderer owns content. Size
and retention policy are the existing `artifacts.max_bytes` and
`artifacts.retention_days` settings.

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

`examples/pdf-walkthrough.rs` does the same for PDFs. With only
`BILIBI_PDF_DEMO_OUT=/absolute/dir` it writes the deterministic fixture PDFs and
prints the renderer refusals (`-- fixtures`); with the disposable-DB variables
above it also generates, stores and reads back an owner-selected PDF and shows
unauthorized, cross-tenant, non-PDF and truncated-output refusals leaving no
artifact behind.
