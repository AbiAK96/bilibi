# Domain parity reference

This harness exports the Bilimbi baseline and the three domain revisions from
§3 of the domains port plan. It uses source archives rather than a Git worktree,
so it cannot move the reference repositories' branches. It runs no unit tests.
Generated references, credentials, PostgreSQL data, schema dumps and screenshots
must stay outside every repository.

```bash
python3 scripts/domains-parity/reference.py export \
  --projects /path/to/firstmate/projects \
  --evidence /path/to/data/bilibi-domains-parity
```

Use a **private PostgreSQL 18 cluster bound to loopback**, with a database named
`bilimbi_domains_parity`. Set `DATABASE_URL` to that database. A long task path
can exceed PostgreSQL's Unix socket path limit; disable Unix sockets with
`pg_ctl ... -o "-h 127.0.0.1 -p <private-port> -k ''"`.

From the exported `reference/` directory:

```bash
# First apply the approved reference repair from Bilibi (see below).
ERL_FLAGS='+S 4:4' mix deps.get
ERL_FLAGS='+S 4:4' mix bilimbi.migrate
ERL_FLAGS='+S 4:4' mix run --no-start ../fixtures.exs
(cd apps/web && ERL_FLAGS='+S 4:4' mix do assets.setup + assets.build)
ERL_FLAGS='+S 4:4' mix run --no-start ../serve.exs
```

`serve.exs` binds HTTPS only on `127.0.0.1:41461` (override `PARITY_HTTP_PORT`).
It disables workers and uses an exported self-signed TLS identity without
registering a shared ingress. The isolated capture browser accepts that local
certificate; no system trust store changes are needed. It leaves the pinned production sources unchanged. `fixtures.exs`
provisions a disposable operator through the real bootstrap API, and creates no
domain records. Credentials are generated into `identity.json` with mode 0600.
Never put that file in Git. `walkthrough.exs` is an explicit skeleton for later
slice proofs, which must use another disposable database.

From Bilibi, keeping `DATABASE_URL` pointed at the reference:

```bash
python3 scripts/domains-parity/reference.py dump --evidence /path/to/evidence
python3 scripts/domains-parity/reference.py capture --evidence /path/to/evidence
```

The dump action writes a complete `pg_dump --schema-only` and 15 module dumps.
Each module has a companion `.functions.sql`: `pg_dump -t` omits functions even
when a table's triggers need them. Apply that companion before the table dump,
with the module's Base/Core and domain dependencies already installed. Table
ownership comes from the pinned owners' Ecto schemas, including Factory's
historical table names. `schema/inventory.json` records the exact tables and
hashes. These are PostgreSQL reference inputs; Rust migration versions and the
new migration ledger remain the port's responsibility.

Capture uses only `chrome-devtools-axi`, with a named session, 1440×1000 viewport,
light theme, and Asia/Kuala_Lumpur timezone. It signs in through the real form,
requires a connected LiveView, and saves an accessibility snapshot beside each
full-page screenshot. Use `--theme dark` for another capture set. The inventory
contains all 44 live routes and five controllers; controller downloads belong to
later populated proofs. The 34 non-parameterized routes retain empty domain data. The ten detail
routes with no fixture use the valid but absent ID `2147483647` and are explicitly marked `missing-record`, so a refusal capture
cannot be confused with a populated detail page.

## Initial composition obstacle

On 16 October 2026 the specified Bilimbi `d48e3423` plus the pinned domain
revisions migrated successfully (15 modules, 120 domain tables), then refused
contribution installation:

> authz contribution from finance/reconciliation verb import is already owned by factory/production_execution

The fallback `2a99148` has byte-identical `ContributionValidator` code. Both
domains declare `import`. A combined runtime therefore needs an explicitly
recorded reference compatibility repair before it can provide screenshots.
Firstmate approved removing Reconciliation's redundant `import` registration
in steering message 001 at 2026-10-16T04:17:53Z. All capability keys are
unchanged; Production Execution still provides the verb. Apply it only to the
exported scratch reference:

```bash
python3 scripts/domains-parity/reference.py repair --evidence /path/to/evidence
```

The action writes `reference-compatibility.patch` and `reference-repair.json`,
including before/after hashes. Never apply this repair to the source domain
repositories. The duplicate registration is a real Bilimbi composition defect
to file separately; it is not a change to the Rust port.


## Browser driver compatibility

Each capture gets a fresh named `CHROME_DEVTOOLS_AXI_SESSION` by default and
explicitly selects a tab. Set `CHROME_DEVTOOLS_AXI_PORT` to a free task port if
the CLI's hashed session port collides with another worker; do not stop that
worker's bridge. Reusing this task's signed-in session for another theme is
supported with `--session`.

The installed CLI's selector-based `page.fill`, `page.click`, and `page.wait`
helpers pass evaluated expressions where its MCP backend expects a function.
The harness uses the working `page.eval` API to send normal input/change/click
events and waits for actual LiveView connection. Login navigation is polled
from the driver because replacing the document destroys its old execution
context. This uses the real sign-in flow and durable cookie throughout.


## Reference failures

By default capture refuses an HTTP error. For an unmodified reference defect,
`--record-reference-errors` keeps the screenshot and accessibility snapshot,
records `outcome: reference-error`, HTTP status and disconnected state in the
manifest, and continues the inventory. A reference error is never certified as
a rendered LiveView. The missing-account Matching page in the pinned baseline
raises `KeyError: :lines`; preserve and report that separately rather than
changing the baseline UI.
