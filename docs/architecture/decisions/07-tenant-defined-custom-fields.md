# ADR 07: Tenant-defined fields stay with their owning records

**Document Type:** Architecture Decision Record
**Status:** Accepted; runtime Grid/Authz providers are implemented; full #131 remains in progress.
**Agents:** codex/gpt-6.1-sol-default (author)
**Scope:** Tenant-defined typed fields on opted-in module record types
**Last Updated:** 2026-10-22

## Context

Modules currently declare record and Grid fields at compile time. Tenants need additional attributes on records without a release or tenant-specific DDL, while retaining the owner's tenant boundary, version checks, audit history, and field restrictions.

## Decision

1. **The owner row stores values.** An opting-in owner adds one `custom jsonb NOT NULL DEFAULT '{}'` object column with an object check and a GIN `jsonb_path_ops` index. The owner migration adds it once. The owner remains responsible for tenant scoping, locking, versioning, and writes.
2. **Definitions are tenant-owned Base data.** `base/custom_fields` stores definitions keyed by tenant, record type, and immutable field key. It exposes a facade for listing, defining, updating, archiving, restoring, and validating definitions. Every write uses `MutationBoundary` and returns `Change` records.
3. **Opt-in is explicit.** An owner contributes a record type, its audit kind, Grid table ID, custom JSON column, and owner capability. Discovery validates the declaration against installed Grid and audit metadata. No other record type is extensible by default.
4. **Grid and Authz own runtime-provider contracts.** Each consumer defines a typed contribution contract for runtime metadata. Custom Fields contributes implementations; Grid and Authz consume the contracts without depending on the Custom Fields crate. Runtime metadata is loaded for a sealed scope and is never cached across requests. The implemented Grid contract describes root custom fields, including reference values as stored target keys. Dynamic relationship paths and target-readable checks remain a follow-up. The Authz contract exposes tenant custom fields to restriction management and audit masking.
5. **Field keys never become SQL identifiers.** Grid resolves custom values through the owner's declared JSON column and a bound JSON key. Typed casts are selected from validated field metadata. List filters, sorting, and projections use the same query cost and timeout bounds as other Grid reads.
6. **Field restrictions reuse Authz storage.** Restrictions keep the existing `(tenant, table, field)` identity. Dynamic custom field IDs use the `cf_<key>` namespace. Authz validates new restrictions against the current tenant catalog, applies existing deny-wins masking/refusal behavior, and maps field restrictions to the opted-in audit record kind.
7. **Choice options belong to the field in this phase.** Codes are stable; labels and order may change; retired options remain valid on stored values but are unavailable for new assignments. Integration with governed pick-lists is a later change.
8. **No reporting or shared export support is added.** Those are separate issues.

## Consequences

- The dependency graph stays acyclic: Custom Fields depends on the Grid and Authz interfaces it implements; neither consumer imports Custom Fields.
- Grid and Authz gain request-scoped runtime catalog APIs, and Authz's restriction page becomes tenant-aware for contributed dynamic metadata while retaining static fields from the existing installed catalog. Grid can project runtime reference keys; it does not expose a path through the target record yet.
- Grid's typed runtime provider feeds `Catalog::for_scope_with_runtime`; projection and text search bind each JSON key, while `query::field_expression` returns key bindings for owner-managed sort/filter statements. Authz's runtime provider feeds its restriction and audit-mask catalog. Providers run on the caller's connection or transaction and fail closed on errors.
- Types are enforced by the owner facade, not by PostgreSQL's JSONB storage. Decimal values are kept as exact strings; dates as `YYYY-MM-DD`; date-times as UTC ISO strings; choice values as option codes; references as target keys.
- JSON expression sorting and filtering may be more expensive than indexed typed columns. Existing Grid admission and statement timeout bounds apply; per-field indexes remain a later, measured optimization.
- Custom field definition changes and each changed custom value are auditable. Restricted values must be removed before rendering or serialization, including agent output and audit history.

## First adopter

Core Company opts in through a new additive migration and uses the shared Base UI controls. Its existing free-form `metadata` JSON remains unchanged. Other modules opt in through their own contributions and migrations.
