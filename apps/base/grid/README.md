# Grid catalog (G1)

Grid owns flexible columns on list pages: the catalog every module declares its
tables into, the paths a person walks through it to pick columns without writing
a join, and (in later slices) the one statement that answers them for the rows a
list already shows. It has no page of its own. G1 is the catalog, the scoped view
of it, and the bounded column-path parser. Query/rollups (G2), lenses (G3) and
per-page arrangement (G4, including the `ui.grid.page_columns` setting) build on it.

## Contributing a table

Depend on `base/grid`, submit one `GridContribution` to `TABLES`
(`bilibi_grid::contribute!("core/x", "x.grid", &GRID)`), and declare:

- a `TableDecl`: stable lowercase `id` (unique across modules), the tenant
  `capability` that reads any of its rows, a `Source`, the `key` field, optional
  `label_field` and `time_field` (a date/datetime), `fields`, and `record_types`
  (the audit `auditable_type` values its rows use, so a restriction reaches their
  audit views; one table claims a type).
- a `Source`: static SQL for **the rows a scope may read**, with the owner's own
  tenant, soft-delete and company filters applied. A tenant source takes bigint
  `$1` (tenant) and may take `$2` (user); a `Source::global` takes none. The grid
  wraps it in a subquery, so every hop of a path carries its owner's boundary and
  Grid never adds a tenant predicate. It must be one `SELECT`/`WITH` statement
  that references `$1` exactly when it is tenant-scoped.
- `Field`s: `Field::new`/`choice`; `column` names the source key when it differs
  from the id; `hidden` fields exist only for joins; `protected` keeps a field
  out of operator restriction (the key, label, time, hidden and join fields are
  protected implicitly).
- `LinkDecl`s: `One` (a foreign key to the target's **key**, or an edge yielding at
  most one row per parent) or `Many` (the reverse, or an attachment table), joined
  `on` two declared fields or through an edge `Source` selecting `from_key` and
  `to_key`. A link may start at another module's table, because the module that
  owns the edge knows it; it must own one end.

Contributions are validated at discovery (duplicate ids, unknown link ends, bad
joins, a link id that shadows a field, wrong source shape, claimed record types)
and the Ready check refuses a table naming a capability Authz does not declare.
Nothing executes a source at boot.

## The scoped catalog

`Catalog::for_scope(client, &scope)` reads fresh grants and field restrictions
(`authz::effective_capabilities`, `authz::fields::restricted`) and keeps only the
tables the actor may read and, within them, only the fields no held role
restricts: any restricting role masks, even alongside grant-all. A link to a
table the actor cannot read is dropped, so such a path is refused exactly like an
unknown one. Platform capabilities need the operator identity. A system identity
has no `Scope` and reads nothing. Build the catalog again wherever the page
refreshes its own capabilities; there is no cache of grants.

The first catalog for a source asks PostgreSQL to describe (not run) its
statement and keeps the selected keys for the process, since the SQL is static.
A declared field its source does not select, a placeholder that is not bigint, or
an edge without `from_key`/`to_key` fails with `CatalogError::Misdeclared`
naming the table and field (the owner must fix it; callers' `?` maps it to an
internal error and logs the cause).

`restriction_catalog` and `restrictable` give the Authz screens the protected-field
catalog as **data**: Authz does not depend on Grid.

## Column paths

`name`, `group.name` (one-links), `movements:count`, `movements.delta:sum`,
`group.items.name:list` (a many-link rolls up; one-links may follow it).
`column::parse` accepts at most 160 bytes, 8 lowercase `[a-z][a-z0-9_]*` segments
and one known aggregate (`count sum avg min max latest list`); anything else is
`Malformed` before any catalog is consulted. A path holds at most one many-link
and must then end in an aggregate; `count` takes no field; `sum`/`avg` need a
number, `min`/`max` an ordered value, `latest` a table with a time field.
`resolve_all` collapses duplicates and accepts at most 32 columns. `suggest` ranks
reachable columns (paths up to three links, 50 results at most, 4096 candidates
at most).

## Reference tables

`reference_groups`, `reference_items` and `reference_movements` (tables
`base_grid_ref_*`, capability `grid.reference.view`) are non-business stock data
Grid owns so later slices have a catalog to run against: a one-link, many-links, a
chained one-link after a many-link, an enum, decimals, dates, and a restrictable
field (`reference_items.unit_cost`). Managers, viewers and staff gain the
capability through the contributed role extension; run `authz-reconcile` after
deploying. The tables are empty unless a walkthrough or fixture fills them.

## Running evidence

No unit tests. On a disposable migrated and seeded database:

```sh
BILIBI_GRID_DEMO=disposable cargo run -p bilibi-web --example g1-grid
```

It fills reference rows (tenants 1 and 2), shows catalogs differ by capability,
direct deny, platform gating and role revocation; resolves valid and refused paths
(typed errors); feeds the parser hostile input; ranks suggestions; hides
`unit_cost` through a role restriction (including an admin holding that role,
revoked live) and shows a stale restriction on a protected field is inert; runs
the source with `$1` for two tenants; and installs deliberately defective
contributions to show discovery refuses each. It removes what it added.
