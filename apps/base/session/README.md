# Base Session

`base/session.sessions` (`Sessions` in `src/lib.rs`) owns the durable session
store: authentication of the opaque cookie token, the operator list and its
termination commands, the termination broadcast, and the scheduled sweep.
Bilimbi's counterpart is `Bilimbi.Base.Session`; the admin screen is
`Session.Web.IndexLive` at `/system/sessions` and the dashboard widget
`Session.Web.DashboardStatsPanel`.

## Guard boundary

`check(client, token)` rechecks absolute and idle expiry, the live tenant and
the Core-owned account verifier without writing session activity. Use it on
background timers, subscription callbacks, socket upgrades and service
revalidation. `activity(client, token)` performs the same checks and renews
only an eligible, still-existing session row. Use it only after accepting
authenticated HTTP navigation or a user event. Invalid events and failed
authorization do not renew activity. Renewals are throttled, and activity takes
a row lock while deciding whether renewal is due; a revocation between lookup
and renewal is refused. Expiry is enforced during authentication and never
depends on the cleanup job.

The idle lifetime and the renewal throttle are the operator settings
`session.lifetime_minutes` (default 120) and
`session.last_activity_touch_minutes` (default 5), declared in
`src/contributions.rs` and read on every check through `Settings::get_global`,
so a change applies to the next request. The throttle never exceeds half the
lifetime (`Policy::from_minutes`). The 24-hour absolute expiry set at sign-in
remains.

## Listing

`/system/sessions` needs the platform operator identity and
`admin.system.session.list`; terminating needs `admin.system.session.manage`
too. The contributed system roles `auditor` and `system_viewer` list sessions.
The list is platform-global, like `count`, because the store is not
tenant-owned; it reads only metadata and never a payload. Each row names its
user through `base/principal_directory`: a user in the operator's tenant shows
their name, one outside it shows `User <id>`, so no name crosses a tenant
boundary. The user column sorts by `user_id`. Search matches the IP address or
user agent recorded once at sign-in by `describe` (the host passes the proxy's
`X-Forwarded-For`/`X-Real-IP` and `User-Agent`; a direct connection records no
address). Rows and termination requests are keyed by a digest of the session id
(`Summary::key`), never by the bearer token.

The dashboard entry `base-dashboard-session-stats` (embed `session.dashboard-stats`,
slot `dashboard.sessions`) shows the open count, reading only while
`admin.system.session.list` is effective and otherwise an em dash.

## Ending a session

`terminate(tx, id, current)` is the operator command: the caller's own session
is refused before any row is read (`Termination::CurrentSession`), the delete is
an audited mutation recording whose session ended and from where (the payload
is marked redacted; the token is never written), and the termination is
announced after the commit. `delete(client, scope, id)` is logout: the holder's
own session, no audit row. `end_user_sessions(sql, tenant, user, except)` runs
inside an owner's transaction (Core User's password change and reset) and
returns the ended ids for `announce` after that commit, so a rolled-back
credential change signs nobody out.

`subscribe_terminations()` is the broadcast the host consumes: every live
socket subscribes once (`apps/web/src/live.rs`) and, when its own session is
announced, sends the browser back through the page it holds and closes. The
page's guard then forwards to sign-in with the expired-session message. A
lagged subscriber may miss an announcement; the next guarded action or 15 s
refresh still refuses an ended session, so promptness, not correctness, depends
on the broadcast. `prune_expired` announces nothing: nothing live can hold an
expired session.

## Cleanup

**Prune expired sessions** (`src/expiry.rs`, `base/session-expiry`, every five
minutes) deletes rows past the absolute expiry or idle beyond the lifetime
setting. Review and enable it at `/system/schedule` before expecting scheduled
cleanup; the enablement and delivery rules belong to Base Schedule.

## Try it

On a migrated disposable database, install the walkthrough fixture
(`cargo run -p bilibi-walkthrough-fixture`; it appoints tenant 1 as the
platform operator, which the production migration never does), run the app
and sign in as `admin@example.test`
(role `admin`, grant-all). Open `/dashboard` for the Sessions strip and
`/system/sessions` for the list. Sign in as another account in a second
browser profile, then terminate its row: that browser's open page is sent to
sign-in at once, and the row disappears. Your own row carries the Current badge
and no terminate control.
