# Person identity and personal contacts

`PersonService` (`core/person.persons`) and the transaction APIs in
`src/identity.rs` own `persons`. They use K0's `PersonInput`, `PersonKind` and
`PersonId`; an identity is never an employee or login account. Core works with
People absent. `/persons`, `/persons/new` and `/persons/{id}` provide the
capability-gated directory, create form and inline record/contact editor.

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

## Person screens and record panels (K8)

The People menu entry is a Core contact directory. It has no employee number,
employment type/status/dates, supervisor, department or automatic account
controls. Human/agent is identity kind; current/retired is identity availability.
Retirement preserves history and is independent of HR employment.

The directory composes shared `ListState` and Grid `PageColumns`, with bounded
25/50/100 row pages, search through visible contacts, kind and retired filters,
name/kind/creation-date sorting and stable ID ties. Search, filters, sort,
pagination and Grid `cols/lens/z/since` state survive reload and Back; column
arrangements use shared per-account Grid persistence. Add personal email/phone
through the notch; fresh restrictions prune walked columns and suggestions.

Creation uses the owner validator, shared form errors, non-binding duplicate
suggestions, and creates no login. Detail fields use shared inline text/choice
editors and per-fact commit states. Enter/blur commits, Escape cancels, nullable
contacts can clear, stale versions refuse, and lost write access keeps the page
with readonly facts and an explanatory outcome. Retirement uses a held record
version and shared confirmation dialog. History is the shared audit embed.

`person.contacts` is a reusable discovered embed. A company, affiliation or
account owner first resolves its relation through its public owner API, then
mounts it with `{panel: <stable anchor>, id: <Person ID>}`. It reauthorizes Person
reads and writes, masks restricted contacts, and edits only personal email and
phone. Never pass a company ID as a Person ID. K19 owns the company affiliation
list/editor and work contacts; no guessed company-to-person link exists here.

Person detail automatically mounts contributed embeds whose context is
`Record { kind: "core/person" }`, except its own `person.contacts`. The path
carries `panel`, `id` and `person_id`; anchors are stable and browser-owned.
Affiliation, Address, User and relationship owners can contribute their panels
without Person importing them or requiring People. Each panel is separately
guarded by the host. HTTP rendering leaves child anchors for live mounting.

The manual `person-contact-panel` example serves an independent record host at
`/person-contact-reference/{id}` so the shared contact embed can be tried outside
Person detail. Run it with an existing migrated/seeded disposable `DATABASE_URL`:

```sh
APP_ORIGIN=http://127.0.0.1:4499 BIND_ADDR=127.0.0.1:4499 \
  cargo run -p bilibi-person --example person-contact-panel --locked
```

Sign in, open the reference route with a Person ID created through `/persons/new`,
edit a personal contact, and verify it in the ordinary Person page. The reference
host resolves the Person through its public API on every update; a production
company host additionally resolves its owned affiliation before supplying that ID.
