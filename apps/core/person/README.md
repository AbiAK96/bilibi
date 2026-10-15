# Person identity and personal contacts (K7)

`PersonService` (`core/person.persons`) and the transaction APIs in
`src/identity.rs` own `persons`. They use K0's `PersonInput`, `PersonKind` and
`PersonId`; an identity is never an employee or login account. Core works with
People absent. K8 supplies list/create/detail pages; this slice has no routes.

The additive module migration creates tenant-scoped bigint identities, personal
email/phone, explicit human/agent kind, version and creation/change metadata.
It changes no demo company/account ID. No HR table exists to map in Bilibi.
No employee number, employment state/dates/type, department, supervisor, account,
password or grant can be supplied in a Person input. It rejects unknown keys.

Names are trimmed and contain 1–160 characters; optional email/phone are trimmed
and blank becomes null. Email permits the source's `local@domain` shape, forbids
whitespace/control characters and is limited to 254 UTF-8 bytes; phone has a
64-character limit. Email/name are deliberately non-unique. `duplicates` matches
exact normalized name/email (case insensitive) or trimmed phone, bounds results
to 1–100, can exclude an existing ID and includes retired identities. It proposes
candidates without choosing, merging or linking them.

Every public read and write gates on contributed `admin.person.*` capabilities.
The manager/tenant-owner roles receive all Person commands, staff can create and
edit, viewer can read/list. Existing installations run explicit
`bilibi-web authz-reconcile` after migration; the walkthrough explicitly
reconciles its disposable fixture before making calls.
`admin.person.system.manage` additionally requires the explicit platform operator.
No Person identity, agent kind or role declaration grants capabilities itself.

`persons` is registered with Grid and Authz. Personal email/phone restrictions
remove values from every read result, including duplicates and agent output.
General search only searches visible contacts; duplicate lookup refuses a
restricted contact criterion to avoid existence probes. A read model's
`restricted` names hidden fields; agent JSON returns `{"restricted":true}`.
The same catalog maps restrictions to Person audit evidence.

`update` replaces visible identity/contact fields and requires the expected
version. A missing/null/blank visible contact clears it. A restricted contact
must be missing/null/blank and is preserved, so an editor can rename a person
without learning or erasing hidden data. Row locks serialize writes; stale
versions fail. `retire` retains the identity, contacts and history while refusing
new edits/proofs/bindings. It is independent of People employment lifecycle.

Writes use `MutationTransaction::mutate`; old/new snapshots and the action
commit atomically. Audit failure rejects the write. `PersonService::subscribe`
receives tenant/Person ID notifications only after commit; dropping/rolling back
a transaction publishes nothing. Consumers filter tenant and reauthorize their
own reads; notifications contain no contacts. Broadcast is process local.

`core.person.list`, `.get`, `.duplicates`, `.create`, `.update`, `.retire` and
the `core.person` guide are contributed to Agent API. Writes use E2's reviewed
plans and their outer transaction, including replay protection. Agent input
schemas use the currently supported schema dialect: omit optional contacts on
create and use empty strings to clear visible contacts on update.

## System identity and optional account binding

`ensure_platform_orchestrator` explicitly provisions Lara Bilimbi as an agent
in the operator tenant, resolved by `platform-orchestrator`, never a numeric ID
or employee number. Provisioning serializes concurrent calls and returns the
same record without overwriting edits. Its system key is not a general create
field, its kind cannot become human, and it cannot retire or use the generic
account-binding seam. Generic agents are ordinary contacts. Provisioning creates
no credentials/account, no automatic login and no Authz system-principal grant.
The original demo AI Agent account remains unlinked; K9/K17 own explicit binding
and bootstrap integration, including any privileged system-account workflow.
The reserved key is globally unique. Moving the operator marker to another
tenant requires explicit identity/reference reconciliation; provisioning refuses
to manufacture a replacement in that tenant.

User owns optional account links (K9); Person does not add competing account
storage or depend on User. `identity::lock_for_account_binding` supplies the
small integration seam: a scoped, live, non-system Person locked on the caller's
mutation transaction. It returns a non-deserializable `BindingIdentity` borrowed
from that transaction with tenant, Person ID and kind, containing no contacts.
The caller authorizes its account operation and proves the Company/Affiliation
eligibility before storing a tenant-scoped optional FK. Keep locks in Company →
Person (ascending IDs) → Affiliation → User order and hold them through commit.
The seam never creates or implies a link, and retired/system identities refuse.
Person creation never writes users, sessions, credentials or grants.

Tenancy's `PartyProofs::person` uses this owner to prove live scoped Person IDs;
that proof conveys existence, not a capability or account binding. Principal
Directory providers name only bounded, live Person/agent candidates within the
request tenant. Agent candidates must have `kind=agent`. Base never reads Core
tables directly; the providers contain the owning queries.

## Runtime walkthrough without unit tests

Use a freshly migrated/seeded disposable database, then run:

```sh
BILIBI_PERSON_DEMO=disposable cargo run -p bilibi-person --example person-walkthrough --locked
```

The example calls real app services and prints records/refusals for normalization,
duplicate candidates, concurrent stale saves, cross-tenant IDs, masked contacts,
live capability revocation, retirement, system-agent protection, optional binding
locks, transactional rollback and a deliberately refused audit action. It also
executes create/update/retire through the real safe-write plan service and prints
replay refusal without printing credentials or plan codes. It creates a temporary
operator marker only in the disposable DB and removes it. No People module,
unit tests, test runner or mocks are involved.
