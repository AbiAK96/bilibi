# Core Geonames (C1)

Global geographic reference data: countries, first-level divisions (Admin1),
postcodes and cities. No tenant column, no owner. Callers use `Geonames`
(registry service `core/geonames.lookup`); table layout and SQL stay private.

## Empty on install

The migration (`migrations/001-geonames.sql`) creates the four tables and **no
rows**. Every lookup answers with an empty result (`None`, `[]`, a zero-total
`Page`) until an operator imports; `Geonames::presence()` reports what is there
and `Presence::is_empty()` is the install state. The importer is C2 (countries,
Admin1, cities) and C3 (opt-in postcode countries); there is no unconditional
seed. Address forms must treat an empty lookup as "type it yourself", not an
error.

## Lookups are bounded

- Page queries (`page_countries`, `page_admin1`, `page_postcodes`) take an
  untrusted `IndexQuery`: page sizes clamp to 25/50/100/300, search text to 255
  characters with `LIKE` metacharacters literal, sort keys resolve through closed
  enums to static SQL, and a malformed country filter matches nothing.
- Lists are capped (`MAX_LIST`); form lookups are capped too (postcode prefix
  10, city names 15, exact postcode places 100).
- Lookups are ungated global reads. Capability checks belong to the screens
  that call them (`admin.geonames.list` for the indexes).

## Catalog contributions

- Authz vocabulary: `admin.geonames.view`, `admin.geonames.list` (tenant) and
  `admin.geonames.update` (platform: it edits a table shared by every tenant);
  `tenant_owner` holds all three.
- Grid: table `countries` (global source, keyed by `iso`, read with `list`).
- Menu entries and routes arrive with the screens (C4).

## Running proof

```bash
DATABASE_URL=<disposable db> BILIBI_DISPOSABLE_GEONAMES_DEMO=yes \
  cargo run -p bilibi-geonames --example geonames-walkthrough
```

Shows the fresh-install behaviour, then lookups over a few hand-inserted rows,
hostile input and the Grid catalog entry.
