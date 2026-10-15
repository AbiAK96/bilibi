# Affiliation owner (K18)

`AffiliationService` (`core/affiliation.affiliations`) owns `person_affiliations`.
Use its `read`, bounded `search`, `create`, `update`, `end` and `unlink` APIs.
They refresh actor/tenant eligibility and recheck contributed
`admin.affiliation.*` capabilities. HTTP/session and bearer adapters still
revalidate their credential on each call. The transaction functions in
`src/identity.rs` let authorized adapters participate in an existing audited
mutation; their caller must supply a freshly verified Scope and matching
mutation context. No API accepts a client-selected tenant.

The additive migration preserves all existing Company, Person and account IDs.
It uses K0 `AffiliationId`, `AffiliationInput`, `PersonId`, `OrganisationId` and
inclusive `EffectiveDates`. Person and Organisation foreign keys include tenant;
`(tenant_id,id,person_id,organisation_id)` is also unique for future scoped
account/Employment references. There is no uniqueness on Person, Organisation,
position or email. A Person can hold several concurrent affiliations, including
several positions at one organisation. Work contacts never replace personal
contacts, merge identities or create accounts/grants. No HR fields or People
dependency are installed.

Optional title/work contacts are trimmed and blank becomes null. Title permits
160 characters, phone 64; email uses the Person owner's shape/254-byte validator.
K0's calendar parser rejects impossible/reversed dates; SQL repeats the date
range and ordering constraints. Null date bounds mean unbounded. General search
is paged 1–100 and can filter Person, Organisation and an inclusive calendar day.
Without a day filter, it returns dated history. It excludes unlinked records
unless `include_unlinked` is explicit, and never searches hidden contacts.

Updates require the expected version. The Person/Organisation endpoints are
immutable because account and Employment references retain this identity.
Create another affiliation to move a person to a different organisation.
`end` sets the inclusive end date; `unlink` marks the retained record without
DELETE, cascade or lost foreign keys. Neither erases contacts or history.
Unlinked records remain readable but refuse edits and new binding proofs.
Reads retain ended, retired-Person and archived-company history; writes refuse
archived/deleted/missing companies and retired or protected system Persons.
Owners supply those rules through their public locking APIs.

Grid/Authz register work email and phone restrictions under
`person_affiliations`. Read/list results remove those values and name them in
`restricted`; a restricted value must be absent/null/blank on update, and the
stored contact is preserved. Title/date edits therefore work without exposing
or erasing hidden contacts. Audit snapshots flatten work fields so the same
catalog restrictions cover audit evidence. Mutations and actions commit in one
`MutationTransaction`; failure rolls back both and publishes nothing.
`subscribe()` publishes tenant, Affiliation, Person and Organisation IDs only
after commit. Consumers filter by tenant and reauthorize; fan-out is in process.
Existing installations run `bilibi-web authz-reconcile` explicitly to add the
new capabilities to contributed roles. Manager/tenant-owner receive all
commands, staff create/update, viewer read/list. Roles never follow from a
business affiliation.

## Account and People integration

Call `AffiliationService::lock_for_account_binding` inside the consumer's
`MutationTransaction`. Its sealed, non-cloneable, non-deserializable
`LockedAffiliation` exposes tenant, acting user, Affiliation, Person and
Organisation IDs without contacts. Validate every supplied Person/Organisation
against these owner-issued IDs and store scoped foreign keys; do not accept a
DTO or matching integer as a proof. The caller independently authorizes its
account operation and holds this transaction through the account write.

People calls `AffiliationService::lock_for_employment` instead. Its distinct
`OwnCompanyAffiliation` is issued only if the affiliation's Organisation equals
the tenant's explicit `tenant_primary_companies` assignment. It never infers our
company from an ID, the first company, actor labels or an external affiliation.
The tenant row lock fences primary-company transfers; owner locks fence company
archive, Person retirement and affiliation update/unlink until commit. Missing
primary assignment, external affiliation, retired/system Person, archived
company, unlinked affiliation or cross-tenant identity refuses the proof.

Use global lock order tenant (for own-company proof) → Company → Person →
Affiliation → User/Employment. When locking multiple identities, sort Company
and Person IDs first. Call the proof service before locking User/Employment.
The proof borrows the outer transaction; consume/check its identity before
mutably borrowing the transaction to record the downstream write. Dropping the
proof releases the Rust borrow, **not** the database locks. Never commit the
proof transaction and write in another one. People still authorizes Employment
and owns its own HR dates/status; affiliation dates do not determine employment
status. Ended affiliation identity remains provable while linked so historical
Employment can refer to it. People must decide its own date eligibility.

K19 adds Person/Organisation affiliation editors and headless
operation discovery/preview/commit adapters using these owner APIs. K18 adds no
new page or separate authorization policy. Core contacts and work affiliations
work with People absent.

## Runtime walkthrough (no unit tests)

Migrate/seed a disposable DB, then run:

```sh
BILIBI_AFFILIATION_DEMO=disposable cargo run -p bilibi-affiliation --example affiliation-walkthrough --locked
```

The command uses real services and prints concurrent external positions,
normalization, independent personal contacts, paged panels, proof refusals,
explicit primary reassignment, tenant isolation, stale saves, field restrictions,
live capability revocation, atomic audit failure, rollback event suppression,
end/unlink retention and archived/retired owner guards. It creates no account,
People table or unit-test infrastructure. Use a fresh disposable database for
repeatable records; do not run the fixture on a shared working database.
