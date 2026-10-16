# Core Address (K11)

`AddressService` (`core/address.addresses`) owns `addresses` and
`address_attachments`. Pages, panels and future headless adapters call its
APIs; no other module reads or writes these tables. Every call refreshes the
actor's scope and rechecks contributed `admin.address.*` capabilities; no API
accepts a client-selected tenant. K12 adds the `/addresses` pages and the two
owner panels on these APIs, and its menu entry with them.

## Address facts and normalization

An address holds label, phone, three street lines, locality, postcode, country
(`country_iso`) and division (`admin1_code`), plus provenance (`raw_input`,
`source`, `source_ref`, `parser_version`, `parse_confidence`) and
`verification_status` (`unverified`, `suggested`, `verified`). Bilimbi's quoted
camelCase columns are snake_case here.

- `AddressService::validate` is the only way to a `ValidAddress`, which every
  store write takes. It trims, turns blanks into `None`, uppercases the
  country, bounds lengths, checks the confidence is a 0–1 decimal with at most
  four places, then checks against Geonames that the country is imported and
  the division belongs to it. Postcode and locality stay free text.
  Failures are `WriteError::Invalid` with per-field messages for a form.
- Country and division are foreign keys into Geonames, which is empty on
  install: an address can have no country until an operator imports countries.
- `suggest`/`options` (`src/location.rs`) port Bilimbi's `LocationSuggestion`
  for the create form, the detail page's grouped Apply and a panel's
  create-and-attach form: a country change clears the dependents; a postcode
  change fills the division and a single unambiguous locality and clears what
  was only suggested; typed values are never replaced. Use them; do not
  re-implement the cascade in a page.

## Owner attachments

Bilimbi's polymorphic `addressables(type, id)` becomes one scoped foreign key
per party kind (K0 contract): an attachment names exactly one Organisation
(`companies`) or Person (`persons`) of the same tenant, with `PartyId` in the
API. One attachment per address and owner carries kinds (`headquarters`,
`billing`, `shipping`, `branch`, `other`), the primary flag, priority (0–32767)
and inclusive `valid_from`/`valid_to` (`EffectiveDates`, null = unbounded).
Several owners may share one address.

- Reads: `attached(owner)` (primary first, then priority, then id),
  `primary(owner)` (Bilimbi `primary_company_address`), `available(owner, q)`
  (the attach picker, 50 rows) and `detail(id)` with country/division names and
  every live owner named through its owner. Archived companies' and retired
  Persons' addresses stay readable; `detail` flags those owners `read_only`.
- Writes: `attach`, `create_and_attach` (a `manual`, `unverified` address
  starting today unless dated), `update_attachment` and `detach`. They are
  writes to the owner, so they need `admin.company.update` or
  `admin.person.update`, and go through the owner's public lock, which refuses
  archived companies (`Archived`) and retired or system Persons
  (`OwnerReadOnly`). Attaching twice is `AlreadyAttached`.
- `update` of an address any archived company uses is refused (`Archived`), as
  in Bilimbi. A Person does not freeze an address: in the party model a Person
  has no employer, and the address may be shared.
- `delete` is a soft delete, refused while any attachment remains (`InUse`).
- Updates take the expected version (`Stale` otherwise); attachments carry
  their own version. Lock order is Company (ascending) → Person → Address →
  Attachment; a write never locks a company after its address.

Work addresses: neither the K0/K18 contract (`apps/core/party/CONTRACT.md`) nor
issue 947 gives an Affiliation an address, so none is linked here. A work
address is the affiliated Organisation's attached address.

Grid/Authz register `phone` as restrictable under `addresses`; reads remove a
restricted phone and name it in `restricted`, a write must not supply it, and
the stored value is kept. Mutations (`address`, `address_attachment`) and the
action commit in one `MutationTransaction`. `subscribe()` publishes the tenant,
address and affected owners after commit only. Grid links: `addresses.country`,
`companies.addresses`, `companies.primary_address` and `persons.addresses`.
Existing installations run `bilibi-web authz-reconcile` to grant the new
capabilities to contributed roles: tenant owner and manager all, staff all but
delete, viewer view/list.

## Platform-operator locale bootstrap

`platform_operator_bootstrap()` (`src/bootstrap.rs`) returns the
`bilibi_locale::Bootstrap` facts for one-time installation-locale inference:
the live operator tenant's primary company, its first address (primary, then
priority, then id) and that country's Geonames name, languages and currency.
Any missing step is `None`, never an error. Base Locale does not query Core;
K15 passes these facts to `Locales::resolve`/`adopt_inferred`.

## Runtime walkthrough (no unit tests)

On a fresh disposable database: `migrate`, `seed`, then import the Geonames
sample (`apps/core/geonames/sample`, served by any static server with the
`export/dump` and `export/zip` layout, `cities15000.txt` and `MY.txt` zipped):

```sh
bilibi-web geonames-import --base-url http://127.0.0.1:PORT/dump \
  --postcodes-url http://127.0.0.1:PORT/zip --postcodes MY --cache-dir DIR
BILIBI_ADDRESS_DEMO=disposable cargo run -p bilibi-address --example address-walkthrough --locked
```

It prints normalization and refusals, the location cascade, CRUD with stale
and cross-tenant refusals, the phone restriction, Organisation and Person
attachments, primary resolution, the bootstrap facts before and after the
operator's address, archived-company and retired-Person freezes, refused and
soft delete, and audit counts. It marks tenant 1 as the platform operator when
none exists, so use it only on a disposable database.
