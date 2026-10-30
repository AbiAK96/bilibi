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

## Proposed contract

1. A schedule definition declares `Reach::Installation` or `Reach::EachTenant`. `Installation` remains the default and preserves current behavior. Reach is part of the definition fingerprint; changing it invalidates the existing review.
2. The scheduler claims one `EachTenant` occurrence in the operator tenant as it does today. A Platform-owned fan-out dispatches tenant jobs; the owner module's worker never enumerates or writes across tenants itself.
3. Per-tenant system work needs a tenant-audience execution capability. The existing `bilibi_schedule::EXECUTE` is platform-scoped and cannot authorize the `scheduler` principal in a customer tenant. The proposed key is `bilibi_schedule::TENANT_EXECUTE`; declarations and provisioning grant it nowhere by default. This follows per-tenant consent, not ADR 12's standing-grant exception. The key remains proposed until the Queue binding design is accepted.
4. Tenant discovery stays in Tenancy. The proposed Schedule-only directory method uses the existing `Tenancy::Authorization` binding to Authz, requires the live caller to be the declared `scheduler` principal in the active platform-operator tenant, checks the `platform_operator` marker, and rechecks the narrowly scoped platform capability `admin.tenancy.schedule.fan_out` for every page. It does not create an `OperatorProof`: that proof is intentionally refused to system principals. `SystemScope` alone is not authority to enumerate tenants.
5. Enumeration must return both `active` and `suspended`, undeleted tenants, in ID order and bounded pages of 200. Active tenants with the per-tenant system grant are eligible for jobs; suspended tenants are counted as `skipped`, and active tenants without that grant as `not_granted`. Deleted tenants are excluded. Fan-out progress makes pages resumable without backfill.
6. For each tenant, a single audited `MutationBoundary` transaction inserts a unique tenant-run row and inserts the child job in Queue: scheduled system jobs use `Queue::enqueue_system`; tenant-user manual jobs use `Queue::enqueue_for`. The scheduled-run uniqueness key is `(occurrence_id, tenant_id)`; the manual-run key is a stable tenant-local request ID. Use conflict-safe insertion (`ON CONFLICT DO NOTHING RETURNING`); if no row was inserted, lock/read the existing row and reconcile its linked Queue job through a status lookup scoped to that tenant. Never blindly enqueue a replacement. If the linked job exists and is nonterminal, retain the link and resume reconciliation. If it is terminal (`completed`, `cancelled` or `discarded`), record the terminal outcome and do not enqueue again. If it is missing, record a failed outcome with a bounded `linked_job_missing` reason and stop; only an explicit audited repair may create replacement work. Retryable jobs remain linked and follow Queue's existing retry policy. An insert or enqueue error rolls back both writes.
7. Queue selects the capability for the trusted execution mode and checks it after rehydrating the user or system identity, before handler invocation. It captures that exact capability in `Execution`; `Execution::begin` re-proves the identity and rechecks the same capability inside the business transaction. It cannot fall back to another allowed capability if the selected grant is revoked. The required gates are platform `bilibi_schedule::EXECUTE` for installation work and operator manual runs, tenant `bilibi_schedule::TENANT_EXECUTE` for scheduled system child jobs, and tenant `admin.schedule.tenant.execute` for tenant-user manual runs. The API may use an actor/audience-aware policy or distinct worker bindings, but the trusted mode must come from the binding/job contract, never request arguments. Schedule also checks the current global definition fingerprint, review, enabled and suppression state through a separately authorized boundary; it must not manufacture an operator scope from a child scope. Owners remain responsible for idempotent effects because Queue is at least once.
8. Forbid-overlap under `EachTenant` is per `(definition key, tenant)`; Installation-reach keeps the current global overlap key. A slow tenant does not block other tenant jobs. A changed, disabled, paused or unreviewed definition prevents unstarted work from beginning. Queue's per-attempt grant and tenant checks cancel pending work after revocation or suspension; completed effects are not rolled back.
9. Tenants are considered at fan-out time. A tenant created later participates on the next occurrence; there is no backfill. Missed occurrences continue to coalesce once at claim time. Tenant-local wall-clock schedules, tenant-specific cron/pause settings and per-company fan-out are out of scope.
10. Operator Run now fans out one manual occurrence. A tenant administrator may request one tenant-local manual run under `admin.schedule.tenant.execute`; that job runs as the requesting person and does not create a system grant. The proposed design records one tenant-local child-run row with `trigger=manual`, the stable request ID, requesting actor, tenant, definition fingerprint and exact Queue `job_id`, committed with the job in the same audited transaction. `Schedule::run` validates that row for the tenant manual path; installation runs continue to validate against `base_schedule_occurrences`.
11. Operator history reports `queued`, `succeeded`, `failed`, `skipped` and `not_granted` per occurrence and supports authorized tenant filtering. The tenant page, gated by `admin.schedule.tenant.view`, returns only that tenant's definitions, grant state and runs. Retention covers global occurrence data and tenant run history.

## Open design decisions before implementation

1. **Queue capability binding:** `base/queue` currently binds exactly one capability to a handler. The required check sequence and mode-specific capabilities are specified above. Decide whether Queue gains an actor/audience-aware typed policy or Schedule registers distinct worker bindings/paths. The selected mode must be trusted binding/job metadata, not request arguments; Queue must preserve the exact selected capability from its pre-handler check through `Execution::begin`. An unchecked `AnyOf` must not blur platform and tenant audiences or let one execution mode invoke another. Until the binding shape is selected, `EachTenant` must not be described as supported by the current single `EXECUTE` binding.
2. **Operator tenant inclusion:** decide explicitly whether the platform operator tenant is itself an `EachTenant` target. The current proposal does not include or exclude it by inference; the directory must expose the operator marker so the chosen policy can be applied and surfaced in counts.
3. **Manual-run occurrence model:** current `Schedule::run` requires an occurrence row linked to the exact Queue job. The global occurrence points to the fan-out job, so it cannot stand in for each tenant child. The proposal is a tenant-local child-run row keyed by a generated child ID, carrying the global occurrence ID when scheduled and a nullable parent occurrence for a tenant manual run. Its trigger, tenant, fingerprint, actor and exact child `job_id` are committed with the job in the same audited transaction. Extend reserved schedule metadata with that child ID; `Schedule::run` validates the matching tenant-local row for each-tenant jobs and preserves the current global-row path for installation jobs. Confirm this row shape and the stale/retry rules before coding.
4. **Per-tenant capability name and lifecycle:** select the tenant-audience capability key and make it clear that the operator's existing platform `EXECUTE` grant does not imply tenant consent. The key and any Queue binding changes must be reflected in vocabulary validation and upgrade notes.
5. **Global eligibility recheck:** child jobs must observe global review, pause and disable changes without gaining a child-to-operator scope conversion. Decide the narrow proof/read boundary and its lock ordering so a stale child cannot start during a concurrent disable or fingerprint change.

The public `Definition` addition is a source-breaking change for Rust struct-literal callers. Update all in-repository callers in the same change and use the clean-break policy for official Domains, including companion PRs and upgrade notes where required. Do not add a compatibility field, alias, wrapper or deprecated copy. The next Schedule migration ID must be selected from the registry: `base-schedule-003-schedule-timing` already exists, so `base-schedule-003-tenant-runs` is unavailable.

## Consequences

- One operator review governs when and what the task runs; each tenant separately decides whether the named system principal may run it there.
- Fan-out can resume after process failure without duplicate child jobs, and each tenant's work, audit attribution, history and failure lifecycle stays tenant-local.
- Schedule and Domains do not gain general tenant-listing authority or perform cross-tenant SQL. Tenancy owns tenant enumeration through a system-only, capability-checked binding; Queue owns per-attempt job authorization.
- Work-calendar arithmetic remains the owner's responsibility under the Core work-calendar contract when available. Schedule does not interpret tenant business time.
- The operator board and tenant page are separate read paths with different scopes; adding those pages must follow shared `base/ui` models and live-page contracts.

## Alternatives rejected

- **One schedule registry entry per tenant:** multiplies cron definitions, review state and fingerprints without changing the task's global timing.
- **Owner module loops over tenants:** grants Domains cross-tenant enumeration and bypasses Queue's tenant scope and retry lifecycle.
- **One operator-tenant job writes into every tenant:** makes cross-tenant writes part of one job, audit context and failure boundary, instead of isolated tenant work.
- **Standing grant for all tenants:** conflicts with the issue's explicit per-tenant consent and silently authorizes work during provisioning.

## Out of scope

- This proposal does not define tenant-local wall-clock firing, per-tenant schedules or per-tenant pause controls.
- This proposal does not define per-company fan-out or make Queue's at-least-once delivery exactly once.
- The runtime and UI behavior is not implemented by this ADR; the record remains Proposed until the owner accepts the decision and the feature is built.
