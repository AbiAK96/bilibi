# Bilibi

A minimal MIT-licensed Rust port of [Bilimbi](https://github.com/BelimbingApp/bilimbi),
the Phoenix implementation of Belimbing. This is a working Companies slice,
not a replacement for the full platform or a bake-off benchmark implementation.

## Run locally

Install stable Rust 1.99, Node.js >=22 with npm, PostgreSQL 18 server and client tools, Caddy, mkcert
and Python 3.11+. Caddy must already be running with its admin API at
`http://localhost:2019`. On Ubuntu/WSL2:

```sh
sudo apt install caddy mkcert libnss3-tools postgresql-18 postgresql-client-18 python3
cargo install cargo-watch --version 8.5.3 --locked
./scripts/start-app.sh
```

Open <https://bilibi.localhost>. This one command checks prerequisites, starts
the private PostgreSQL 18 instance in `.local/` (or uses your `DATABASE_URL`),
discovers deep modules, compiles, migrates, seeds, configures local HTTPS and
watches for changes. The first login is `ai@agent.my` with password `bilibi-dev`.
Cargo diagnostics and wall-clock build times stay visible;
`target/cargo-timings/cargo-timing.html` shows the compile breakdown.
Changes under `apps/` (Rust, templates, CSS/JS, migrations and discovered module
crates), workspace manifests/lockfile, and `scripts/` trigger a rebuild and app
restart. In development, an open page shows “Rebuilding…” while the server is
restarting, reconnects with backoff, then reloads the same URL automatically.
Release builds reconnect with the same backoff, remount the page in place so
drafts survive, and say how many requests were in flight when the connection
dropped.
Restart `./scripts/start-app.sh` after changing startup environment variables
such as `DATABASE_URL`, `APP_ORIGIN`, `BIND_ADDR`, `PG18_BIN`, or
`BILIBI_DEV_PORT`; watched file changes do not reload the launcher's environment.
Ctrl-C stops this app; the shared Caddy service continues running.

HTTPS uses a mkcert leaf certificate loaded into the running shared Caddy through
its admin API. The launcher updates only its tagged route, TLS policy and
certificate, then reapplies them on each start. It keeps certificate files in
`.local/https`, makes no shared Caddy file changes and uses the default backend
port 4300 (or the next free port).

The launcher initializes and reuses its private PostgreSQL 18 data directory in
`.local/postgres18` on port 5438. Its development role and database are both
`bilibi` with password `bilibi`. Set `DATABASE_URL` to use another database.
For manual HTTP development, use:

```sh
export DATABASE_URL='postgres://bilibi:bilibi@127.0.0.1:5438/bilibi'
export APP_ORIGIN='http://localhost:3000'
python3 scripts/discover-modules.py
cargo run -p bilibi-web -- migrate
cargo run -p bilibi-web -- seed
cargo run -p bilibi-web -- run
```

Open <http://localhost:3000>. Use tenant `1` and one of these accounts with
password `bilibi-dev`:

| Email | Role | Access |
|---|---|---|
| ai@agent.my | manager | View and edit all company fields |
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

Outgoing mail goes to `.local/mailbox/` (one file per message, newest 100) unless
`MAIL_HOST` configures an SMTP relay; see [Mail](apps/base/mail/README.md).
Email verification links also need `BILIBI_USER_SIGNING_KEY` (64 hex characters).

`BIND_ADDR` defaults to `127.0.0.1:3000`. `APP_ORIGIN` must exactly match the
browser's origin, without a trailing slash. POST requests and WebSocket upgrades
require that Origin. HTTPS origins set Secure session cookies; terminate TLS at
a reverse proxy when using HTTPS. PostgreSQL connections use `NoTls`, intended
for this local slice or a trusted private connection. No migrations or seeding
run automatically on app boot.

## What is ported

- Server-held random sessions, Argon2id passwords, sign out, a 24-hour expiry,
  and throttled activity touches. Session and active tenant/user are refreshed
  for every authenticated request, live event and push.
- The sign-in page is the homepage (`/`, with `/login` as an alias), on
  Bilimbi's centered credential layout with its forgotten-password and reset
  cards: `/forgot-password` always confirms without saying whether the account
  exists, and the emailed `/reset-password/{token}` link replaces the password
  once and ends every session of the account. A completed reset and an expired
  session each reach the sign-in page as a one-shot flash cookie that names a
  message, never carries one. Signed-in visitors are forwarded to the
  workspace. `/settings/password` is the account's self-service change inside
  the shell: a live page whose form submits as a module event, requiring the
  current password and ending the account's other sessions. The forms carry a
  Tenant ID because the credential lookup is still tenant + email: the global
  login and Person/Affiliation identity binding follow K9, and sign-in has no
  attempt throttle yet.
- Roles, role capabilities and direct grants; direct denies override every
  allow, including admin `grant_all`. Each capability check writes a tenant-scoped
  decision-log row, whether allowed or denied. Unknown capabilities cannot be
  granted by persisted data unless declared in the contributed vocabulary.
  Platform capabilities additionally require explicit operator identity. Named
  system principals need a declared capability and a durable tenant grant;
  anonymous system identities always deny. See [Authz](apps/base/authz/README.md).
- Any held restricting role masks the field, even alongside an unrestricted
  or admin role. Masked values are removed before rendering and cannot be edited.
  The shared masking API supports other tables, JSON projections and audit values.
  `/authz/field-restrictions` administers them: roles, then tables, then fields
  as chips, committed together; see [Authz](apps/base/authz/README.md).
- Tenant-scoped, soft-delete-aware Companies list with search, status filter,
  sorting and 25/50/100-row pages; detail with inline edits and archived read-only
  behavior. A row lock plus version check rejects stale edits.
- Old/new company values and actor recorded in `base_audit_mutations`, plus a
  committed `company.update` row in `base_audit_actions`, in the same transaction
  as the save. An audit error rolls back the company update and notification.
  Notification creation/read changes are audited too. Session creation, deletion
  and activity touches are authentication housekeeping. Evidence is append-only;
  secrets are redacted by field name; the read-only SQL console engine records
  every command. See `apps/base/audit/README.md`.
- Warm stone shell, lime orientation markers, compact ruled tables, header,
  sidebar, account/tenant disclosure and a notification bell, drawn from
  Bilimbi's `DESIGN.md` and shell components. System fonts provide a local
  fallback for Instrument Sans; no remote font dependency.
- A company save creates a durable notification for its actor and pushes the
  bell to that user's connected pages. The bell opens Bilimbi's dropdown of
  recent notifications (mark all read, open one, view all); `/notifications`
  lists them with All/Unread/Read tabs, mark read and paging. A notification
  wakes only its recipient's bell and list (matched by tenant **and** user), so
  the page underneath, and any draft on it, is not re-rendered.

The public sign-in, forgotten-password and reset pages, their submissions and
static assets have no signed-in actor and therefore no capability decision;
the password settings page needs a session and no capability. Sign out checks `session.logout`;
company operations check `companies.view`/`companies.update`; bell operations
check `notifications.view`/`notifications.update`. Invalid wire input is rejected.
Permissions, tenant/user status and restrictions are read afresh, not cached.

## Live pages and module boundaries

The stack is axum on Tokio, tokio-postgres with a bounded deadpool pool, embedded
SQL migrations and Askama templates. Askama plus the browser's native WebSocket
API was chosen because both are small, established tools: compiled templates
escape data by default, and axum already provides socket support. A short shared
browser adapter queues intent and patches server-rendered HTML into the page by
element id. There is no SPA
framework or client model. A pinned Tailwind build produces the committed CSS;
Node is needed only to rebuild assets, not to run the Rust app. See the
[UI asset contract](apps/base/ui/assets/README.md) for sources and licenses.

Each connection owns its route, list parameters, loaded redacted records, and
rendered page. Sort/search/page events reload through the same domain APIs as
HTTP. List state is reflected in the URL for reloads and sharing. Detail saves
reload the rendered page, avoiding client-side permission logic. Notifications
publish after commit and re-render only their recipient's bell and list, through
authorization. A 15-second refresh also catches account suspension, session
expiry, logout, changed roles, and notifications written by another process. Denied access clears the displayed page and closes the
socket. The browser patches replies into the existing DOM, so focus, scroll, unfinished
drafts (with their version, so concurrent changes still conflict) and iframe
documents survive bell and refresh patches; every intent is queued and
acknowledged by request ID, so rapid input is never dropped. A lost connection
reconnects with backoff and remounts the same URL; development reloads after the
watcher restarts the server. See [docs/live-pages.md](docs/live-pages.md).

```text
apps/base/            Base composition root
apps/core/            Core composition root
apps/domains/         empty Domain composition root
apps/extensions/      empty Extension composition root
apps/base/module_registry  discovered module graph and typed service registry
apps/base/database    pool, migrations ledger, errors, mutation boundary
apps/base/audit       audit evidence schema, capture, redaction, SQL console engine
apps/base/security    compatibility facade for Session, Tenancy and Authz
apps/base/authz       contributed vocabulary, live grants, decisions, masking
apps/base/grid        contributed table/field/link catalog, scoped and restriction-aware
apps/base/mail        validated outgoing mail: SMTP relay or dev mailbox files
apps/base/notifications  transactional notification sink and adapter readiness
apps/base/workflow    status definitions, append-only history, owner-transaction transition kernel
apps/base/ui          shared shell, credential layout and cards, styles, small live browser adapter
apps/core/user        credentials, password reset/verification, settings pages, sessions, notifications, owned SQL
apps/core/company     Companies API, owned SQL, page templates
apps/web              executable host, route/socket transport, explicit seed
```

Base does not depend on Core. Core depends on Base and declared public Core APIs
(Company uses the Base notification sink; User binds its adapter at boot). Each deep module is a Cargo
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
module-contributed SQL; it keeps no module or migration list. Company resolves the Base notification boundary; User installs its persistence
adapter and contributes the bell as the `shell.notifications` embed. Boot refuses missing or duplicate bindings. The same bus serves
both notification writes and Web subscribers. Typed route/embed/seed contributions
carry module/file/line provenance and are validated into an immutable catalog
(see `apps/base/module_registry/README.md`). Unknown consumers, wrong payload
types, duplicate contribution IDs and missing service references fail discovery.
Service factories can resolve only their own module and declared direct dependencies.
Domains and Extensions are empty composition roots, ready for later modules.
All application SQL binds data and includes tenant predicates; session-token
bootstrap and the explicit demo seed/migration commands are the exceptions.

## Checks

Bilibi has no unit tests. The checks are the compiler, Clippy and running the app.

```sh
python3 scripts/discover-modules.py --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --release --locked
```

## Left out

Other business modules, user/role administration, registration, the password
reset, forgot-password and verification pages (their services exist, see
`apps/core/user/README.md`),
saved column layouts, tiling, settings, impersonation, audit and decision-log
screens, distributed notification fan-out, production login throttling and
operational hardening. Broadcasts are process-local; use one app process for this
slice. A reconnect/periodic refresh reads durable notification counts. This
creates a fresh schema and does not adopt a Belimbing/Laravel database. No agent
API, plugin system, Laravel compatibility layer, or optional domain modules.
