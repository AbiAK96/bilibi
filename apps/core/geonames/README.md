# Core Geonames (C1)

Global geographic reference data: countries, first-level divisions (Admin1),
postcodes and cities. No tenant column, no owner. Callers use `Geonames`
(registry service `core/geonames.lookup`); table layout and SQL stay private.

## Empty on install

The migration (`migrations/001-geonames.sql`) creates the four tables and **no
rows**. Every lookup answers with an empty result (`None`, `[]`, a zero-total
`Page`) until an operator imports; `Geonames::presence()` reports what is there
and `Presence::is_empty()` is the install state. There is no unconditional
seed: the import below is an explicit operational action. Address forms must
treat an empty lookup as "type it yourself", not an error.

## Import

```bash
cargo run -p bilibi-web -- geonames-import
cargo run -p bilibi-web -- geonames-import --datasets countries,admin1
cargo run -p bilibi-web -- geonames-import --cache-dir /var/lib/bilibi/geonames --force
```

Countries, Admin1 and cities (`cities15000`) import by default, in that order;
postcodes are opt-in per country (C3). `ReferenceImport` (registry service
`core/geonames.import`) is the API: `run` for the operator command, `run_as`
for a screen, which checks `admin.geonames.update` on the sealed scope first.

- **Streaming, atomic per dataset.** Files are read line by line and written in
  500-row `unnest` upserts inside one transaction. A database failure in any
  batch rolls the whole dataset back (`ImportFailure::Database`); a file with no
  valid row is refused (`NoValidRows`) so a truncated payload never passes for
  an empty import. Blank, comment and malformed lines, and rows the schema
  would refuse, are counted as skipped. The run stops at the first failing
  dataset; datasets before it stay committed.
- **Local names survive.** A re-import refreshes every upstream fact but never
  replaces a country's `country` or an Admin1 `name`.
- **ETag cache.** Downloads go through `base/integration` (`stream`, recorded as
  system exchanges) into `--cache-dir`, default `GEONAMES_CACHE_DIR` or
  `$TMPDIR/bilibi/geonames`; use a durable directory in production. A cached
  file with an ETag is revalidated (`If-None-Match`, 304 keeps it); one without
  an ETag is trusted for 7 days with no request. A body lands in a temporary
  file renamed over the cache only after a 2xx. When the server is unreachable
  or answers 429/500/502/503/504, the existing cache is imported and the report
  says so with its age; `--force` re-downloads and never falls back.
- **Cache recovery.** Before each download the cached payload and ETag are
  copied aside; if the download, extraction or import fails, the known-good
  cache is restored and any extracted zip entry deleted.
- `--base-url` points at a mirror with GeoNames' `export/dump` layout.

Geonames tables are reference data imported wholesale, so the import writes
outside mutation capture, as Bilimbi excludes these schemas from audit.

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

```bash
DATABASE_URL=<disposable seeded db> BILIBI_DISPOSABLE_GEONAMES_DEMO=yes \
  cargo run -p bilibi-geonames --example geonames-import-walkthrough
```

Serves `sample/` (small GeoNames-format payloads, see its README) from a local
ETag fixture and runs the importer through first import, 304 revalidation,
local-name preservation, 503/unreachable fallback, `force`, a no-valid-rows
payload, a database failure in a late batch, the ETag-less cache lifetime and
the capability gate. To try the command itself offline, zip
`sample/cities15000.txt` as `cities15000.zip` beside the other two files,
serve that directory over HTTP and pass `--base-url`.
