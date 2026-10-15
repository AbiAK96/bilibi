# Perf history

`base/perf` records redacted request, live-event, job and process samples into
`base_perf_samples` without ever delaying the work it measures. Reading the
history for the Performance page is O4; this module owns recording, storage,
retention and the read queries.

The service is `base/perf.recorder` (`Recorder`). Re-exported as
`bilibi_base::perf`.

## Observing work: use `observe`, not your own timer

```rust
let response = recorder
    .observe(Kind::Request, Some("/companies/{id}"), handler, |response| Finish {
        outcome: Outcome::Ok,
        response_bytes: response.body().size_hint().exact(),
    })
    .await;
```

`observe` times the future, attributes the database round trips made inside it
(`bilibi_database::timing`) and hands one `Sample` to `submit`. Identities are
route templates, event names or worker ids; a concrete path, query string or
anything failing `valid_identity` is run unobserved. Dropping the future
records a `cancelled` sample. Adapters (`apps/web/src/perf.rs`, the queue
observer in `jobs.rs`) only say which identity and how it ended.

## Why telemetry cannot slow business work

- `submit` never awaits, never touches the database or settings and never
  fails. It returns `Accepted`, `Ignored` (off, below the minimum duration,
  outside the sample rate) or `Dropped` (queue full).
- The policy (`perf.enabled`, minimum duration, sample rate) is atomics
  refreshed in the background every 10 s, so a settings change is noticed by
  every process without a hot-path read.
- One writer task (`Recorder::run`, started by the web app) batches rows on its
  own connections with a 5 s bound. A failed or timed-out batch is counted in
  `Stats::dropped`; it is never retried or buffered without limit.
- `health_status` reports `Degraded` when samples were lost or the queue is
  full, and `Unavailable` when the writer is not running or the store cannot be
  read; the request path never consults it.

## Process facts

Each minute the writer stores a `runtime` sample: resident memory, CPU time,
threads, open file descriptors, Tokio queue and task counts, and database pool
use. These replace
Bilimbi's BEAM facts (schedulers, run queue, atoms, ports).

## Retention

`perf.history.keep_days` (30) and `perf.history.max_rows` (200 000) bound the
table. The daily `base/perf-retention` Schedule definition prunes both, and the
writer re-enforces the row cap every 100 accepted rows. Retention runs even if
recording is switched off.

## Running proof

`examples/perf-walkthrough.rs` drives observation, redaction, settings policy,
queue saturation, an unreachable store, regressions and retention against a
disposable database. It refuses to run without `BILIBI_DISPOSABLE_PERF_DEMO=yes`
because it truncates `base_perf_samples`:

```sh
BILIBI_DISPOSABLE_PERF_DEMO=yes DATABASE_URL=postgres://... \
  cargo run -p bilibi-perf --example perf-walkthrough
```
