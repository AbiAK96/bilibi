# Tenancy identity boundary

Use `Authentication` and `Scope` from `src/lib.rs` for authenticated identity. Raw route IDs and K0 party IDs carry no authority. Scope and Actor have private identity fields, no deserialize implementation and no public constructor; the display fields are never policy input. A proof applies to one freshly guarded unit of work, not a permanent credential.

`base/tenancy.authentication` owns a once-bound `ActorVerifier`. Core User installs the account adapter during registry boot; missing and duplicate bindings fail startup. The `ProofClient` read adapter keeps owner checks on the caller's connection or transaction. Base has no SQL against Core account/company/person tables. `Authentication::authenticate` is an authentication-edge API: supply only identities extracted from validated durable credentials, never request IDs. It establishes identity and grants no capabilities. `Scope::mutation_context` supplies F2 attribution from that identity.

`base/tenancy.parties` owns optional `OrganisationProvider` and `PersonProvider` bindings. K1/K7/K9 install owner adapters using K0's `bilibi-party-ids`. Missing providers refuse all target proof requests. Providers must prove live, eligible targets in the exact scope, including tenant and access checks; IDs and business relationship roles never grant authority. An organisation/person proof retains its scope and typed target ID. The account adapter (`bilibi_user::identity`) proves each stored Person/affiliation/company link through its owner on every authentication, so an unlinked account carries no facet and a frozen one authenticates nowhere. This boundary neither fabricates Person data nor depends on People.

`prove_operator` returns a sealed proof only when the live scope tenant is explicitly recorded in the singleton `platform_operator` table. Numeric tenant IDs and `grant_all` have no operator meaning. Migration `005-tenancy-operator` leaves the marker unset; K17 owns bootstrap. Operator proof establishes identity, not a permission grant: use Authz `authorize_operator` for an operation that requires both. The migration does not replay the historical tenancy schema in `001-base`.

Named system principals, impersonation and delegated job rehydration require the corresponding P6/Q2/K14 owner adapters; an unverified worker or system identity cannot construct an authenticated Scope through this API.

## Tenant administration and `/tenancy/tenants`

`admin::Tenants` (`base/tenancy.tenants`, `src/admin.rs`) is the tenant
administration service: `list`, `count`, `fetch` and the audited `create`
(mutation `tenancy.tenant.create`, change kind `tenancy.tenant`). Every method
takes an explicit `OperatorProof`; a tenant id, `grant_all` or demo tenant 1 is
never that proof, and `create` re-proves the identity on its own transaction.
Callers receive `TenantIdentity` (Bilimbi `Tenancy.Identity`), never the row;
its `is_platform_operator` is the `platform_operator` marker read with the row.
A refused create is `CreateOutcome::Refused` with the source's field errors
(blank or over-long name, unknown status, missing parent), never an `Error`.
Migration `009-tenancy-hierarchy` adds the source's `parent_id` and timestamps;
the operator marker stays the explicit singleton of `005`.

The capability keys are `admin::{LIST, CREATE}` (`admin.tenancy.tenant.list`,
`admin.tenancy.tenant.create`), both platform capabilities. Authz declares them
on Tenancy's behalf, because the vocabulary type is Authz's and Tenancy sits
below it. The route `/tenancy/tenants` (`src/web.rs`, ported from Bilimbi
`TenantsLive`) is `OperatorCapability(LIST)`, so the host proves identity and
capability on every request and live event; the page decides `CREATE` before
showing the Add Tenant dialog and again before writing. That decision goes
through `Authorization` (`base/tenancy.authorization`), a once-bound
`OperatorAuthorizer` that Authz installs at boot with its `authorize_operator`
(both requirements, independently logged); missing or duplicate bindings fail
startup. A module below Authz that needs a capability decision uses this seam
rather than importing Authz or re-deriving operator standing from an id.

To try it on a demo database, appoint the demo tenant explicitly (K17 owns the
real bootstrap; migrations appoint nobody):

```sh
psql "$DATABASE_URL" -c "INSERT INTO platform_operator(singleton,tenant_id) VALUES(true,1) ON CONFLICT DO NOTHING"
```

Then sign in to tenant 1 as `admin@example.test` (grant-all) and open
Administration → Tenancy → Tenants. Without the marker the same account is
refused (403, and the menu branch stays hidden); with it, an account that holds
`LIST` but not `CREATE` sees the list without the Add button and is refused the
create with a flash. Deleting the marker row restores the demo's non-operator
state.

