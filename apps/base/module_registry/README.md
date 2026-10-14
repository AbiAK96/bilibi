# Registry contracts

Deep modules register `Module` descriptors and `Contribution::new` entries with
`inventory`. Use these contracts rather than adding host-owned module lists or
reading another module's private storage. `Services::install` sorts and validates
the graph, validates contributions, builds services, checks adapter readiness,
and publishes a read-only `Services::catalog()` only after successful boot.

`src/contributions.rs` defines shared `Route`, `Embed`, and `Seed` payloads and
the `ROUTES`, `EMBEDS`, `SEEDS` keys. Each entry includes its stable contribution
ID and `Provenance { module, source: file!(), line: line!() }`. Duplicate IDs within
a consumer, unknown consumers/owners, incorrect payload types, invalid routes,
duplicate route patterns and seed versions, and unknown/undeclared service
references fail discovery. Routes describe method/path, access, framing and a
service operation. Embeds describe slot/context, access and a service operation.
These are metadata contracts; Web's route/event adapter owns execution and must
apply guards. Seed execution and ledger tracking belong to Database; a seed
selects Production/Demo and SQL or a named service operation.

A new consumer (Menu, Settings, Grid, Queue, Agent API, etc.) registers
`Consumer::new::<OwnerPayload>("base/owner.catalog", "base/owner", validator)`.
The owning validator sees the complete sorted `&[Entry]`, including an empty
catalog, and reads its typed payload with `entry.get::<OwnerPayload>()?`.
Validation must be deterministic and perform no IO. Contributors supply static
payloads; registry validation never calls Core business APIs. Catalogs preserve
provenance and enforce the same type when retrieved with `entries::<T>(key)`.
Consumers own semantic checks and cross-entry rules beyond generic duplicates.

Service factories may resolve only their own module and declared direct
module dependencies. `bootstrap.*` values are explicit host-supplied
infrastructure (currently the database pool). Base cannot depend upward on Core.
Base-owned provider boundaries can use `Ready` to reject unbound adapters after
all factories finish; `apps/base/notifications/src/lib.rs` demonstrates this.
Company records through that sink inside the company transaction. User binds
its adapter at boot, audits the notification inside the transaction, and emits
the bell after commit. Do not import User from Company or import Core from Base.

This slice registers existing Company routes and User's shell bell. It does not
replace host dispatch or add a production seed runner; those consume these
contracts in the following foundation slices. No unit tests are used.
