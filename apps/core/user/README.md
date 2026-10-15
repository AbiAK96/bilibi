# User credentials

`core/user` owns accounts, credentials, notifications and the account's own
appearance and pins: the password and email services, the account's
self-service settings pages that call them, the `/notifications` page and the
top-bar bell (the `shell.notifications` embed), the last two in `src/inbox.rs`
over `src/notifications.rs`. Other modules notify through `base/notifications`
(`NotificationBoundary::notify` with a typed `Notice` and recipients, inside
their own transaction); run `examples/notifications-walkthrough.rs` against a
disposable database to see the after-commit fan-out. The anonymous sign-in,
forgot-password and reset-password
pages belong to the Web host, as Bilimbi's do.

## Services (`core/user.accounts`, `UserService`)

| Method | Purpose |
|---|---|
| `login` / `logout` | Tenant + email credential lookup and server session (unchanged) |
| `change_password(token, csrf, current, new)` | Signed-in change. Needs the current password; ends the account's other sessions |
| `request_password_reset(tenant, email)` | Always `Ok` for unknown, frozen or throttled accounts. Mail is sent after returning |
| `reset_password(tenant, email, token, new)` | Consumes the one-time token, replaces the password, ends every session |
| `send_email_verification(token, csrf)` / `verify_email(link)` | Signed link to the account's current address; clicking proves mailbox control |

Refusals are `CredentialError` variants a page maps to a field error or flash
(`IncorrectPassword`, `Policy`, `InvalidOrExpiredToken`, ...). Unknown, frozen,
expired, reused and wrong-email tokens are one indistinguishable
`InvalidOrExpiredToken`.

Use `password::{hash, verify, check_policy}` for any password work instead of
calling `argon2` directly. It runs Argon2id on the blocking pool, bounds input
(8 characters to 1024 bytes) and `verify(.., None)` costs the same as a real
check, so timing does not reveal which emails exist.

## Appearance and pins

The signed-in account's theme, time-display mode, locale and sidebar pins
(`src/preferences.rs`; Bilimbi `DisplayPreferences`, `Pins`). Every call takes
the sealed session `Scope`, so a request can never name another account, and
all of it is keyed by the current tenant + account IDs: membership is untouched
and nothing moves when K9 binds Person/Affiliation.

| Method | Purpose |
|---|---|
| `presentation(scope)` | One snapshot: theme, time-display `mode` + company `timezone`, resolved `locale`/`language` and the account's own locale choice |
| `save_preference(token, csrf, kind, value)` | `theme` (`light`/`dark`/`system`), `timezone` (`company`/`local`/`utc`) or `locale` (empty follows the installation). Unsupported kinds and values are `PreferenceError::Invalid`, never guessed |
| `list_pins(client, scope)` / `toggle_pin(..)` / `reorder_pins(..)` | Pin or unpin a page (audited), renumber the order (a display preference, so not audited) |

Theme is the Settings value `ui.theme` at the account's scope (`system` is the
absence of a row); time-display mode and locale are the existing
`ui.timezone.mode` and `ui.locale` values written through their owners
(`base/datetime`, `base/locale`). Pins are `user_pins`: one row per normalized
URL per account (path with trailing slash, fragment and host dropped, query
sorted; `url_hash` is md5 of it), at most 100, written under a lock on the
account's row. A URL that would leave the site is refused. Pins to a page this
deployment no longer serves stay stored and are not drawn.

Web adapters (`apps/web/src/shell.rs`): `POST /preferences` (`{kind,value}`),
`GET /api/pins`, `POST /api/pins/toggle` (`{label,url,icon}`) and
`POST /api/pins/reorder` (`{pins:[{id}]}`), each with Origin and CSRF checks and
a 422 `{error}` for a refused value. The shell renders the account's theme
(`<html data-theme>` on first paint), mode and pins (`data-pins`) itself, and
`/settings/appearance` (`src/appearance.rs`) is the same three choices as a
live form. Impersonation is not ported; when it lands it must refuse these
writes for the account being viewed, as Bilimbi does.

## Token storage

`password_reset_tokens` has one row per account (primary key tenant + user), so
it can never grow past the user count. Only the SHA-256 of the 256-bit random
token is stored; the plaintext exists in the emailed link alone. A request is
throttled to one per 60 seconds, a link lives one hour, and each request
deletes expired rows. Consuming a token deletes its row inside the audited
transaction, so a racing second use loses. If delivery fails the row is
removed so the person can ask again.

Verification links are stateless: `v1.{tenant}.{user}.{issued}.{hmac}` signed
with `BILIBI_USER_SIGNING_KEY` (64 hex characters; separate from the Settings
and Media keys). The MAC covers the current email, so changing the address voids
links already sent. Without the key `send_email_verification` and `verify_email`
answer `VerificationUnavailable`.

Emailed links point at `APP_ORIGIN`: `/reset-password/{token}?tenant=…&email=…`,
served by the Web host, and `/verify-email/{link}`, whose page belongs to a
later slice.

## Pages (`core/user.pages`)

`/settings/password` (`src/web.rs`, `templates/password.html`) and
`/settings/appearance` are the signed-in
account's password change inside the shell, on the settings layout of
`templates/settings.html` (Bilimbi `SettingsComponents.settings_layout`); later
settings pages (profile, appearance) render through the same macro and are
shown as unavailable until they land. Agent connections links to
`core/agent_access`'s own page, as in Bilimbi. The page is live: the form
submits as the `change-password` event carrying its field values, the state
keeps the field errors of the last attempt (`can't be blank`, the password
policy, `does not match password`, `is incorrect`), and a success or failure
is a flash in the page until dismissed. The route needs a session and no
capability; the current password proves the holder inside `change_password`.

## Audit

Credential writes are audited as kind `user` with marker values
(`"password_hash":"replaced"`), never a hash; the audit layer redacts the field
name as well. Reset and verification are attributed to the account that proved
the token.

## Known gaps

Identity is still tenant + email, and the archived-company freeze applies only
through tenant and account status. The global login and Person/Affiliation
binding follow K9. No throttle exists on guessing a password at login.

## Walkthrough

`cargo run -p bilibi-user --example credentials-walkthrough` against a
disposable database after `migrate` and `seed` (set
`BILIBI_DISPOSABLE_CREDENTIALS_DEMO=yes`, `BILIBI_MAIL_DIR` and
`BILIBI_USER_SIGNING_KEY`). It prints every outcome above and reads the dev
mailbox to follow the links.

## Saved database queries (C9; pages follow in C10)

Resolve `database_queries::QueryService` as `core/user.database_queries`. Use
`list`, `get(QueryKey::Id|Slug)`, `create`, `update`, `delete`, `duplicate` and
`execute` from [src/database_queries.rs](src/database_queries.rs), rather than
running editor SQL or writing `user_database_queries` directly. Each call
revalidates the account and explicit platform-operator identity. Reading and
executing require `admin.system.database-table.list`; saving, duplicating and
deleting require `admin.system.database-table.edit`. Admin grants alone do not
make a tenant the operator. The UI must also gate routes and events (C10).

Saved records belong to `(tenant_id, user_id)` on every operation, including
integer IDs; a foreign owner's record is never returned. `QueryInput` contains
name (required, 150 characters), SQL (required, 100,000 characters), optional
slug, description, prompt and icon. A missing slug generates a unique readable
slug on creation and preserves the slug on update. Creation/duplication slug
selection is serialized per owner. Updates/deletes require the returned
`version` and refuse stale drafts with `Error::Conflict`. All saved-query
writes use the mutation boundary, so audit refusal rolls back the data. List
search/sort/page are SQL operations; page sizes are bounded to 300.

`execute` accepts `ConsoleRequest` (SQL, display name, page, per-page) and
`ConsoleOptions` (named parameters, optional output-column sort and direction).
Use `extract_named_parameters` to build the editor's parameter fields. It skips
quoted strings/identifiers, escaped strings, dollar quotes, casts and comments.
Named `:value` parameters bind once by name in PostgreSQL's inferred text input
format, so `:id::bigint`, `:active::boolean`, dates and text work without SQL
interpolation. An absent/None parameter binds NULL. Parameter values never enter
the audit payload. `ConsoleAnswer` has the outcome, optional result and durable
action ID; a refused/failed command has no result. The editor passes unsaved SQL
and retains its draft across result paging. Output sorting is accepted only for
an exact returned column, duplicate column names retain separate values, and
arbitrary-precision JSON numbers preserve exact PostgreSQL numeric output.

The executor uses PostgreSQL `READ ONLY` with a 10-second statement timeout,
repeatable-read count/page snapshot, at most 1,000 result rows per page and
explicit rollback. Text guards are advisory; a function that tries a hidden
write is refused by PostgreSQL with SQLSTATE 25006. Every execution success,
refusal (including authorization) and failure is recorded before an answer;
audit failure returns no rows. Audit contains bounded SQL/name, status, guard
and count, never result rows or raw database error details (which could quote
data or parameter values). It cannot record during a total database outage and
therefore fails closed. Error details remain available in the operator's answer.

Run `cargo run -p bilibi-user --example database-queries-walkthrough` against a
**disposable** migrated/seeded database with `DATABASE_URL` and
`BILIBI_DISPOSABLE_QUERY_DEMO=yes`. This opt-in manual fixture prints CRUD,
concurrent slug creation, stale refusal, owner/tenant isolation, parameters,
column preservation, pagination, SQL/DB refusals, timeout, audit rollback and
private-result exclusion; it is not a unit-test suite. It explicitly makes
fixture tenant 1 the operator, grants fixture capabilities and installs private
fixture functions/triggers. Never run it against normal data.
