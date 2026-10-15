# Schedule definitions and review

Schedule holds installation-global recurrence definitions and the operator's
decision about them. This slice (Q3) covers definitions, fingerprint review,
suppression and the cron/zone arithmetic. Occurrence claiming, the Queue
insertion and run history are Q4; the operator board is Q5. Nothing here runs work.

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
(no database). `review` exercises review, suppression, fingerprint staleness,
authorization and audit on a disposable database (`BILIBI_DISPOSABLE_SCHEDULE_DEMO=yes`,
`DATABASE_URL`, after `bilibi-web migrate` and `seed`).
