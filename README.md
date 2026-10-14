# Bilibi

A minimal MIT-licensed Rust port of [Bilimbi](https://github.com/BelimbingApp/bilimbi),
the Phoenix implementation of Belimbing. This is a working Companies slice,
not a replacement for the full platform or a bake-off benchmark implementation.

## Run locally

Install stable Rust (1.91 or newer), Docker with Compose, PostgreSQL client tools,
Caddy, mkcert and Python 3.11+. On Ubuntu/WSL2:

```sh
sudo apt install caddy mkcert libnss3-tools postgresql-client python3
cargo install cargo-watch --locked
# Enable systemd on WSL2, then start the existing shared Caddy service once.
sudo systemctl start caddy
./scripts/start-app.sh
```

Open <https://bilibi.localhost>. This one command checks prerequisites, starts
local PostgreSQL with Compose (or uses your `DATABASE_URL`), discovers deep
modules, compiles, migrates, seeds, configures local HTTPS and watches for changes.
It generates an owner-readable demo password in `.local/seed-password`, or accepts
`SEED_PASSWORD` in your environment. Cargo diagnostics and wall-clock build times
stay visible; `target/cargo-timings/cargo-timing.html` shows the compile breakdown.
Rust/templates/assets rebuild and the server restarts on change. Ctrl-C stops
this app; the shared Caddy service continues running.

HTTPS follows Bilimbi's mkcert/shared-Caddy approach. The leaf certificate and
key are installed under `/etc/caddy/bilibi/certs`; each site has its checkout
owner line. An advisory lock shared with Bilimbi protects port allocation.
Existing Caddy imports and sites are preserved; reserved/listening ports are
skipped. Deleted Bilibi checkouts' sites are pruned. Changes are backed up,
validated as the Caddy user and rolled back on validation or reload failure.
`BILIBI_DEV_HOST` and `BILIBI_DEV_PORT` pin an explicit host/port; otherwise a
second checkout gets a distinct hostname and free port. The default backend port
is 4300. HTTPS trusts the developer's mkcert CA, including the Windows CurrentUser
root store on WSL2; only the public CA certificate crosses into Windows. WSL NAT
allows the Windows gateway as well as loopback, while refusing remote LAN clients.
Caddy installation is the only privileged step, invoked via sudo by the launcher.

For manual HTTP development, use:

```sh
docker compose up -d --wait
export DATABASE_URL='postgres://bilibi:bilibi@localhost:5433/bilibi'
export APP_ORIGIN='http://localhost:3000'
python3 scripts/discover-modules.py
cargo run -p bilibi-web -- migrate
read -rs -p 'Demo password: ' SEED_PASSWORD; echo
export SEED_PASSWORD
cargo run -p bilibi-web -- seed
unset SEED_PASSWORD
cargo server
```

Open <http://localhost:3000>. Use tenant `1` and one of these accounts with the
password you seeded:

| Email | Role | Access |
|---|---|---|
| manager@example.test | manager | View and edit all company fields |
| staff@example.test | staff | View and edit, with tax ID and revenue restricted |
| viewer@example.test | viewer | Read only; email, phone, tax ID and revenue masked |
| admin@example.test | admin | All capabilities; any additional restricting role still masks fields |

Tenant `2` has its own `manager@example.test` and a private company. Tenant IDs
are explicit at login; requests and events derive the tenant from the session,
never from client page parameters. The seed adds 62 companies to tenant `1` so
pagination can be tried. Seeding and migrations are explicit, transactional and
idempotent; re-seeding preserves passwords and company edits, and ensures the
demo roles and assignments exist.

`BIND_ADDR` defaults to `127.0.0.1:3000`. `APP_ORIGIN` must exactly match the
browser's origin, without a trailing slash. POST requests and WebSocket upgrades
require that Origin. HTTPS origins set Secure session cookies; terminate TLS at
a reverse proxy when using HTTPS. PostgreSQL connections use `NoTls`, intended
for this local slice or a trusted private connection. Supply database credentials
through the environment. No migrations or seeding run automatically on boot.

## What is ported

- Server-held random sessions, Argon2id passwords, sign out, a 24-hour expiry,
  and throttled activity touches. Session and active tenant/user are refreshed
  for every authenticated request, live event and push.
- Roles, role capabilities and direct grants; direct denies override every
  allow, including admin `grant_all`. Each capability check writes a tenant-scoped
  decision-log row, whether allowed or denied. Unknown capabilities cannot be
  requested through the typed API.
- Any held restricting role masks the field, even alongside an unrestricted
  or admin role. Masked values are removed before rendering and cannot be edited.
- Tenant-scoped, soft-delete-aware Companies list with search, status filter,
  sorting and 25/50/100-row pages; detail with inline edits and archived read-only
  behavior. A row lock plus version check rejects stale edits.
- Old/new company values and actor recorded in `audit_mutations` in the same
  transaction as the save. An audit error rolls back the company update and
  notification. Notification creation/read changes are audited too. Session
  creation, deletion and activity touches are authentication housekeeping.
- Warm stone shell, lime orientation markers, compact ruled tables, header,
  sidebar, account/tenant disclosure and a notification bell, drawn from
  Bilimbi's `DESIGN.md` and shell components. System fonts provide a local
  fallback for Instrument Sans; no remote font dependency.
- A company save creates a durable notification for its actor and pushes the
  bell to that user's connected pages. Clicking the bell marks that user's
  notifications read. Subscribers are matched by tenant **and** user.

The public login page, login submission and static assets have no signed-in
actor and therefore no capability decision. Sign out checks `session.logout`;
company operations check `companies.view`/`companies.update`; bell operations
check `notifications.view`/`notifications.update`. Invalid wire input is rejected.
Permissions, tenant/user status and restrictions are read afresh, not cached.

## Live pages and module boundaries

The stack is axum on Tokio, tokio-postgres with a bounded deadpool pool, embedded
SQL migrations and Askama templates. Askama plus the browser's native WebSocket
API was chosen because both are small, established tools: compiled templates
escape data by default, and axum already provides socket support. A short shared
browser adapter sends intent and replaces server-rendered HTML. There is no SPA
framework, client model, or Node build step.

Each connection owns its route, list parameters, loaded redacted records, and
rendered page. Sort/search/page events reload through the same domain APIs as
HTTP. List state is reflected in the URL for reloads and sharing. Detail saves
reload the rendered page, avoiding client-side permission logic. Notifications
publish after commit; their recipient reloads through authorization. A 15-second
refresh also catches account suspension, session expiry, logout, changed roles,
and missed bell events. Denied access clears the displayed page and closes the
socket. The browser preserves unfinished inline drafts across bell patches,
retaining their version so concurrent changes still conflict. Reload to reconnect
after a lost connection.

```text
apps/base/            Base composition root
apps/core/            Core composition root
apps/domains/         empty Domain composition root
apps/extensions/      empty Extension composition root
apps/base/module_registry  discovered module graph and typed service registry
apps/base/database    pool, migrations ledger, errors, mutation audit primitive
apps/base/security    session/scope guard, capabilities, restrictions
apps/base/ui          shared shell, styles, small live browser adapter
apps/core/user        credentials, session actions, notifications, owned SQL
apps/core/company     Companies API, owned SQL, page templates
apps/web              executable host, route/socket transport, explicit seed
```

Base does not depend on Core. Core depends on Base and declared public Core APIs
(Company calls User's transactional notification API). Each deep module is a Cargo
crate below one of the four roots, preserving Bilimbi's composition topology.
`python3 scripts/discover-modules.py` discovers its Cargo manifest and generates
composition dependencies/linkage before Cargo resolves packages. The launcher
runs this automatically; CI checks that composition files are current.

Each deep module submits one `bilibi_module_registry::Module` descriptor through
`inventory`, naming its stable ID, layer, dependencies, embedded migrations and
service factories. Runtime discovery rejects missing/duplicate modules, upward
dependencies, cycles, duplicate migrations and conflicting service names. It
orders Base → Core → Domains → Extensions and dependencies before consumers,
then initializes typed services. The host resolves named services and runs
module-contributed SQL; it keeps no module or migration list. Company resolves
User's notifications through this registry, so both use the same bell bus.
Domains and Extensions are empty composition roots, ready for later modules.
All application SQL binds data and includes tenant predicates; session-token
bootstrap and the explicit demo seed/migration commands are the exceptions.

## Checks

Use a disposable PostgreSQL database: integration tests create and remove an
isolated schema for each test. They require `TEST_DATABASE_URL` and fail if it
is absent, rather than silently skipping database checks.

```sh
export TEST_DATABASE_URL='postgres://bilibi:bilibi@localhost:5433/bilibi'
python3 scripts/discover-modules.py --check
python3 scripts/test-dev.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Tests execute PostgreSQL, real HTTP and WebSockets, including allow/deny logs,
tenant isolation, direct-deny precedence, multi-role redaction, revoked live
permissions/sessions, pagination/search/sort, escaped HTML, CSRF/Origin checks,
concurrent saves, audit-failure rollback and user-scoped bell pushes. Registry
tests prove discovery/service initialization, dependency ordering and invalid-graph
refusal. Launcher tests redirect every privileged path to `/tmp`, use no sudo,
and verify allocation, pruning, rollback, build/watch orchestration and generated
Caddy syntax with the real consumer when installed. GitHub
Actions runs the same checks with a PostgreSQL 16 service.

## Left out

Other business modules, user/role administration, password reset/registration,
saved column layouts, tiling, settings, impersonation, audit and decision-log
screens, distributed notification fan-out, production login throttling and
operational hardening. Broadcasts are process-local; use one app process for this
slice. A reconnect/periodic refresh reads durable notification counts. This
creates a fresh schema and does not adopt a Belimbing/Laravel database. No agent
API, plugin system, Laravel compatibility layer, or optional domain modules.
