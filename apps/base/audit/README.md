# Audit evidence contract

`base/audit` records durable mutation and action facts in `base_audit_mutations`,
`base_audit_actions`, the mutable bookmark table `base_audit_action_metadata`
and the diagnostics table `base_audit_capture_failures` (migration
`006-audit-evidence`). Rows outlive actors and subjects: no foreign keys to
tenants, accounts or records. PostgreSQL refuses UPDATE, DELETE and TRUNCATE on
evidence, stamps `recorded_at` from `clock_timestamp()` and fills a missing
`occurred_at`. The frozen demo ledgers `audit_mutations` and
`bilibi_action_commits` were carried over once as `source = 'legacy'` rows and
receive nothing further.

## Capturing business writes

Do not insert audit rows from an owner. Write through
`bilibi_database::MutationBoundary` (`base/database.mutations`) and return every
affected record as a `Change`; `AuditCapture` is bound to that boundary during
service installation and runs inside the owner's transaction. A refused audit
row, an unrecognised principal or a failing commit-time action row refuses the
business commit: there is no best-effort mode. Boot fails when nothing is bound.
Before refusing, the capture writes a diagnostics row on another connection with
identity and error type only, never values or messages.

A `Change` becomes `created`, `updated` or `deleted` from its old/new objects; a
change with two empty objects writes nothing. Bulk writes arrive as one slice
through `Capture::mutations` and are inserted in chunks of 1,000 rows per
statement. Values are stored in full, with NUL stored as `␀`. Redaction is by
field name on every captured record (`GLOBAL_REDACTIONS`: `password`,
`password_hash`, `remember_token`, `secret`, `api_key`, `token`, `payload`),
plus per-record-kind fields an owner contributes with
`Contribution::new(REDACTIONS, ..., &Redaction { kind, fields })`. The change is
recorded; the value appears as `[redacted]`. Widen the policy rather than masking
at a call site, and name non-secret fields differently.

## Attribution

`Attribution::from_context` maps the F2 `Context` principal kinds `user`,
`agent`, `guest`, `console`, `scheduler`, `queue`, `system` and `unknown`.
`actor_id` names the account acted as; `impersonator_id` names the operator
behind an impersonated session and must be a user; a `system` principal with a
non-zero id becomes `system/0` plus `system_principal`. Any other kind, or a
non-numeric user/agent id, is refused. `Attribution::from_scope` is the sealed
session identity; K14/Q2 supply impersonation and job rehydration contexts.
The committed action row carries `event = Context::action`, so name actions as
`owner.operation` (`company.update`), the way Bilimbi names explicit actions.

## Explicit actions and the console

`record_action(client, &attribution, request, event, payload, retained)` records
attempted, refused and failed operations on the caller's own connection, outside
any rolled-back transaction. `RequestInfo` carries IP, URL and a user agent cut
to 80 characters; free text in payloads goes through `bounded_text` (2,000
characters with a marker).

`console::execute_readonly(db, &scope, request, info)` is the read-only SQL
console engine: operator proof, empty/statement/keyword text guards, then a
PostgreSQL `READ ONLY` transaction with a 10 s statement timeout, count, column
names and at most 1,000 rows per page. Every command is one
`database_query.executed|refused|failed` action row with the SQL as typed and the
outcome; result rows never reach the record, and a command whose record cannot
be written returns no answer. C9/C10 own saved queries, parameter binding and the
screen; they call this function rather than running SQL themselves.

`record_history` and `recent_actions` are tenant-scoped reads for P12/P13; those
slices own the capability vocabulary, filters, field withholding and screens.

## Runtime evidence

Demonstrated without unit tests on a disposable database upgraded from the
pre-P5 schema; see Firstmate task data `data/bilibi-p5/` (reset script, proof
fixture, logs): legacy carry-over, service save with shared trace, redaction and
NUL handling, 2,500-row bulk capture, system/impersonator/queue attribution,
refused unknown kinds, refused mutation and action rows leaving company and
notification state unchanged with diagnostics written, append-only evidence, every
console outcome recorded, and an HTTP/browser smoke of the running app.
