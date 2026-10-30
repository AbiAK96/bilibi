# ADR 17: One global schedule, fanned out under per-tenant consent

**Document Type:** Architecture Decision Record
**Status:** Proposed
**Agents:** codex/gpt-6
**Scope:** How `base/schedule` dispatches one recurring definition to tenant-local Queue jobs
**Last Updated:** 2026-10-30

> Source: [issue 138](https://github.com/BelimbingApp/bilibi/issues/138). The existing Schedule and Queue contracts are in [`apps/base/schedule/README.md`](../../../apps/base/schedule/README.md) and [`apps/base/queue/README.md`](../../../apps/base/queue/README.md).

## Context

`base/schedule` currently stores one installation-global definition and claims an occurrence in the platform operator's tenant. Its worker is queued in that tenant. A Domain cannot enumerate tenants: tenant listing requires an `OperatorProof`, and direct SQL against Tenancy's tables would break ownership and authorization boundaries. `Queue::enqueue_system` already runs one job as a named principal in one tenant and re-proves tenant activity, principal declaration and durable grant on every attempt.

Report schedules, privacy retention sweeps and tenant-local operational jobs need the same recurrence to run separately against each tenant's records. Copying one cron definition per tenant would duplicate operator review and fingerprint state. A worker that reads or writes all tenants itself would bypass Queue's sealed tenant scope and make one tenant's failure block the others.

## Decision

1. A schedule definition declares `Reach::Installation` or `Reach::EachTenant`. `Installation` is the default and preserves current behavior. Reach is included in the definition fingerprint; changing it invalidates the existing review.
2. The scheduler claims one `EachTenant` occurrence in the operator tenant as it does today. It hands that claim to a Platform-owned fan-out worker; the owner module's worker is never run as the cross-tenant dispatcher.
3. The fan-out obtains active, undeleted tenant identities through a narrow Tenancy-owned boundary, in ID order, in batches of 200 per transaction. It records `skipped` for suspended tenants and `not_granted` for tenants without an explicit durable `scheduler` grant for `bilibi_schedule::EXECUTE`. No declaration, role or provisioning step creates that grant. This follows per-tenant consent, not ADR 12's separate standing-grant exception.
4. Fan-out persists progress and one unique child run per `(occurrence, tenant)`. Retrying a batch is safe. Enqueuing a child job and recording its tenant-local run state happen in the same tenant-scoped `MutationBoundary` transaction, attributed to `scheduler`; the child job is enqueued with `Queue::enqueue_system` in that tenant.
5. A child attempt executes only after Queue re-proves its live tenant, declared principal and tenant grant. The Schedule boundary separately checks the current global definition fingerprint, review, enabled and suppression state in the operator tenant; it must not manufacture an operator scope from a child tenant scope. The owner's existing validated scheduled-worker contract runs with the child execution's sealed tenant scope. Owners remain responsible for idempotent business effects because Queue is at least once.
6. Forbid-overlap under `EachTenant` is keyed by `(definition key, tenant)`. One tenant's unfinished work does not prevent other tenants from proceeding. Installation-reach overlap retains its current global key behavior.
7. Operator review, enable/disable, pause/resume and fingerprint changes remain global. A changed, disabled, paused or unreviewed definition prevents unstarted child work from beginning. Revoked grants or suspended tenants prevent their pending child jobs from running through Queue's normal `actor_refused` path. Completed effects are not rolled back.
8. Active tenants are considered at fan-out time. A tenant created later participates on the next occurrence; there is no backfill. Missed occurrences continue to coalesce once at claim time. Tenant-local wall-clock schedules, tenant-specific cron/pause settings and per-company fan-out are out of scope.
9. Operator Run now fans out one manual occurrence. A tenant administrator with `admin.schedule.tenant.execute` may queue one definition for their own tenant as the requesting person. That user-authorized path is separate from the system principal's scheduled-work grant and creates no such grant.
10. Run history carries tenant identity. Operator reads show `queued`, `succeeded`, `failed`, `skipped` and `not_granted` counts per occurrence and allow authorized tenant filtering. The tenant page (`admin.schedule.tenant.view`) returns only the caller's tenant's definitions, grant state and run rows, and explains when a grant is missing. Retention covers global occurrence data and tenant run history.

The public `Definition` addition is a source-breaking change for Rust struct-literal callers. Update all in-repository callers in the same change and use the clean-break policy for official Domains, including companion PRs and upgrade notes where required. Do not add a compatibility field, alias, wrapper or deprecated copy. The next Schedule migration ID must be selected from the registry: `base-schedule-003-schedule-timing` already exists, so `base-schedule-003-tenant-runs` is unavailable.

## Consequences

- One operator review governs when and what the task runs; each tenant separately decides whether the named system principal may run it there.
- Fan-out can resume after process failure without duplicate child jobs, and each tenant's work, audit attribution, history and failure lifecycle stays tenant-local.
- Schedule and Domains do not gain general tenant-listing authority or perform cross-tenant SQL. Tenancy owns tenant enumeration; Queue owns per-attempt system authorization.
- Work-calendar arithmetic remains the owner's responsibility under the Core work-calendar contract when available. Schedule does not interpret tenant business time.
- The operator board and tenant page are separate read paths with different scopes; adding those pages must follow shared `base/ui` models and live-page contracts.

## Alternatives rejected

- **One schedule registry entry per tenant:** multiplies cron definitions, review state and fingerprints without changing the task's global timing.
- **Owner module loops over tenants:** grants Domains cross-tenant enumeration and bypasses Queue's tenant scope and retry lifecycle.
- **One operator-tenant job writes into every tenant:** makes cross-tenant writes part of one job, audit context and failure boundary, instead of isolated tenant work.
- **Standing grant for all tenants:** conflicts with the issue's explicit per-tenant consent and silently authorizes work during provisioning.

## Not decided here

- This proposal does not define tenant-local wall-clock firing, per-tenant schedules or per-tenant pause controls.
- This proposal does not define per-company fan-out or make Queue's at-least-once delivery exactly once.
- The runtime and UI behavior is not implemented by this ADR; the record remains Proposed until the owner accepts the decision and the feature is built.
