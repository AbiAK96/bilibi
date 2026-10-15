# Core Company

`apps/core/company` owns the Organisation of the approved party model
(`apps/core/party/CONTRACT.md`): the `companies` table, its reference catalogs
(legal entity types, department types), a company's departments, the tenant's
primary company and the provisioning commands. The public API is the
`core/company.companies` service (`CompanyService`) and the free functions of
`organisation`, `reference_types`, `departments` and `primary`; table layout
and SQL stay private. The demo list and detail pages (`web.rs`) remain until
K3 ports Bilimbi's screens.

## Schema (K1, `migrations/004-organisation.sql`)

Additive over the frozen demo migration: every existing company keeps its
id, tenant and `/companies/{id}` URL, and `OrganisationId` is that id.
`companies` gains Bilimbi's columns (`parent_id`, `code`, `legal_name`,
`registration_number`, `legal_entity_type_id`, `jurisdiction`, `website`,
`scope_activities`, `metadata`, `updated_at`). `code` is backfilled from the
name as Belimbing slugs it (`organisation::slug_code`) and is **unique within
a tenant**: Bilimbi's compatibility constraint spans tenants, which would let
one tenant learn another's codes. A parent is a company of the same tenant
(`(tenant_id, parent_id)` foreign key) and never the row itself.

`company_legal_entity_types` and `company_department_types` are global
catalogs with no tenant column (code unique, `version` for optimistic
updates). `company_departments` carries `tenant_id` with a scoped foreign key
to its company, one row per type per company, and `head_affiliation_id`: the
head's affiliation with the company under the party model, which K18 binds
with a foreign key and an API. `tenant_primary_companies` is the explicit
tenant → primary company relationship; nothing infers it from an id or the
oldest company. The migration also advances the `companies` identity past
rows inserted with explicit ids.

## Capabilities and reach

`contributions.rs` declares Belimbing's keys: `admin.company.view`, `.list`,
`.create`, `.update`, `.delete` and `admin.company.tenant-wide.manage`, and
extends `tenant_owner` (all), the demo `manager` (all), `staff` (view, list,
update) and `viewer` (view, list). The demo seed grants the same set;
`bilibi-web authz-reconcile` reconciles an older database. A write to a
company or into it is authorized with `organisation::authorize_target`: the
capability now, and either the company the actor is affiliated with
(`Scope::actor().organisation()`, bound by K9) or the tenant-wide reach for
any other company of the tenant. The demo pages still ask the two-segment
`companies.view`/`companies.update` keys until K3 rebinds them.

## Writes

Every write takes the sealed scope, checks its capability at the moment of
the call and records its rows through the caller's `MutationTransaction`
(`tx.mutate`); the service opens one transaction per call and commits only a
saved outcome. A refusal (`CreateOutcome::Refused`, `Outcome::Refused`) lists
`FieldError`s in the source's wording and is an outcome, never an `Error`;
`WriteError` names `NotFound`, `Archived`, `InUse` and `Stale`.

- `organisation::create` / `preview` (Bilimbi `create_company/3`,
  `preview_create_company/2`): trims and defaults (`status` active, code from
  the name), refuses a field the actor's role restricts, proves the parent,
  legal entity type, jurisdiction (`bilibi_geonames::country_known`, so no
  jurisdiction is recordable until countries are imported) and code
  uniqueness on the write's transaction under a per-code advisory lock.
- `organisation::require_writable` is the one archived check: a live company
  whose status is `archived` is read-only for good, for its own facts and
  every record it owns. Reads never ask it.
- `organisation::Provider` is the `OrganisationProvider` Tenancy's
  `PartyProofs` proves targets with: a live, unarchived company of the
  actor's tenant, bound at the service factory.
- `reference_types`: list/get, create, update (versioned), toggle active and
  delete, which refuses a type a live company or a department uses (`InUse`).
- `departments`: list, available types, create, status update (versioned)
  and delete, each beginning with the writable-company guard.
- `primary::assign` locks the tenant row, then the company row, judges the
  company under that lock (archived refused) and assigns or transfers.

Inline edits on the demo detail page (`save`) now accept code, legal name,
registration number, jurisdiction and website with the same rules; `status`
stays an inline choice there until K2 lands the lifecycle.

## Agent operations

`agent_operations.rs` registers `core.company.list` and `core.company.get`
(reads) and `core.company.create` (a reviewed safe write: preview shares
create's validation and code check without reserving it, commit writes
through the plan's transaction) plus the `core.company` guide. A restricted
field arrives as `{"restricted": true}`. The handlers read through a pool
bound once by the service factory.

## Provisioning

```sh
cargo run -p bilibi-web -- tenant-provision --tenant-name "Acme tenant" --company-name "Acme Sdn. Bhd." --company-code acme
cargo run -p bilibi-web -- platform-provision --company-name "Example Operations" --company-code example_operations
```

`primary::provision_tenant` atomically creates a tenant and its primary
company; `provision_platform_operator` resolves the explicit operator tenant
or creates and marks it, and re-running creates neither a second operator
nor a second company. Both are deployment writes recorded as one retained
`company.*.provisioned` audit action by the `provision` system principal,
through Tenancy's `admin::provision_tenant` / `provision_platform_operator`
seams. Ids come from PostgreSQL.

## Runtime inspection

Migrate and seed a disposable database, then run

```sh
BILIBI_COMPANY_DEMO=disposable cargo run -p bilibi-company --example k1-organisation-walkthrough --locked
```

It prints identities and tenancy proofs for the seeded companies, refused and
accepted creates, the administration search with and without restricted
columns, the reference catalogs, departments on company 1, the archived
refusals, primary assignment, the agent operations and a plan commit with
replay, and both provisioning commands. It creates a tenant and, when none
exists, the platform operator. No unit tests belong here.
