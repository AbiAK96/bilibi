# Authorization owner contract

Authz owns capability vocabulary, decisions, grants and field withholding.
Party relationship roles are unrelated to authorization roles. Base never queries
Core account/company tables: Tenancy's account verifier supplies live account
eligibility at the authenticated edge and for grant targets.

Register a `VocabularyContribution` through the registry's `VOCABULARY` consumer.
Contribute domains, verbs, capability keys (with an explicit `platform` flag),
role definitions/extensions, and named system-principal capability bounds.
`Vocabulary::from_catalog` validates the complete vocabulary. Installed Authz also
validates route, embed, menu and settings capability references during boot.
Duplicate capability/principal keys, conflicting role definitions, malformed keys,
and undeclared domain/verb/grant references refuse installation. Capability entries
carry contributor provenance. Existing two-part demo keys remain supported; new
owners should use the source's `domain.resource.action` form.

To extend a seeded role, contribute the same code, canonical name and grant-all
flag, with additional capability keys. Capabilities are unioned across owners.
Deploying a contribution does not change stored role grants automatically:

```sh
cargo run -p bilibi-web -- authz-reconcile
```

Reconciliation serializes runs, upgrades existing demo roles in place, and
synchronizes contributed system-role metadata/capabilities. It preserves role IDs,
assignments, custom roles, direct grants/denies and field restrictions. It removes
obsolete grants from contributed roles, and clears redundant grants on grant-all
roles. Run it again after installing contributions or provisioning new tenants.
`seed-production` also supplies the initial versioned reconciliation through the
Database seed ledger. Neither path runs at app boot. The current demo tenancy
schema requires one role instance per tenant; company-owned custom role binding
remains K15's owner adapter, rather than a Base query of Core tables.

Use `authorize_key` (or the compatibility `Capability` enum) for enforcement and
`decide` for a reasoned decision. Unknown keys deny even grant-all holders.
A contributed platform capability requires the persistent `platform_operator`
identity **and** a grant; tenant 1 and grant-all have no implicit operator power.
Direct deny wins over direct allow, role grants and grant-all. Permission reads
are fresh. Reauthenticate/revalidate a retained Scope at every request/event/job
edge; the sealed scope is identity for one unit of work, not a permanent login.
Decisions record reason and principal kind using the caller's connection; decisions
inside an owner transaction follow that transaction's commit/rollback.

`base/authz.grants` exposes audited custom-role creation, role assignments, custom-role capability
changes, direct overrides, and system grants. It checks the administrator and
principal through the live owner verifier. System roles are immutable through
interactive role-grant writes; reconciliation is their explicit update path.
Capture failure or cancellation prevents commit. No browser-provided actor ID is
accepted as attribution. Callers cannot pass a SystemScope to grant administration.

Named system principals require an installed declaration and a durable grant in
the execution tenant. `SystemScope::rehydrate` is a trusted worker/maintenance
edge, never a browser constructor. Anonymous system identities always deny;
declaration alone grants nothing. Removed declarations make old grants inert.
Named identities hold no user roles, gain no grant-all bypass, and obey platform
gates. The current storage seam follows the demo's tenant-scoped authorization;
company/organisation resource binding is supplied by K15/K9 alongside the User
binder, using Tenancy owner proofs. Queue execution must rehydrate at execution,
rather than retaining a Scope captured when enqueued.

`/authz/field-restrictions` (Administration › Authorization › Field Restrictions,
`src/restrictions_web.rs`, Bilimbi `FieldRestrictionsLive`) is the operator's
screen for `base/authz.field_restrictions`: one row per restricted field, a
dialog that asks for the roles, then the tables, then the fields as chips and
commits the fields picked together as one transaction, a smaller dialog per row
to change its roles or remove it, and the shared confirmation before removal.
The route and every live event require `admin.authz.field.manage`; the service
re-checks it inside each write, so a grant revoked while the page is open is
refused and the host clears the page. Rows whose table the installed catalog no
longer offers (the demo's `companies` restrictions until Company contributes a
Grid table) stay listed by id, can be removed and cannot be re-restricted.

`restrictions_for` and `fields::restricted` read restrictions for any table ID;
any held restricting role masks, even alongside an admin. `fields::mask` removes
restricted fields before JSON serialization, including arrays, audit old/new
objects and headless projections. `fields::refuse_attempts` refuses any submitted
restricted key regardless of its value. Owner-typed read models must also clear
those values before rendering (Companies already does). Named system identities
hold no roles and therefore have no role-based field restrictions. P8 owns the
protected-field catalog contract and restriction administration; Grid supplies
that catalog as data, with no Authz→Grid dependency.

## Running evidence

No unit tests. Use a private migrated/seeded database and run:

```sh
BILIBI_AUTHZ_DEMO=disposable cargo run -p bilibi-web --example p6-authz
```

This explicit runtime walkthrough prints role/direct grant decisions, live
revocation, manager seed extension, unknown-key denial, admin-plus-viewer masking
on Companies and a second table, restricted-null refusal, operator/grant
independence, named/anonymous/cross-tenant system decisions and an audit-failure
rollback. It temporarily appoints demo tenant 1 as operator and installs an audit
refusal trigger, then removes both; never run against a real database. Decision
and audit rows remain as evidence. It requires the real installed owner services.
Compile with the workspace discovery/fmt/clippy checks; do not add a test runner.
