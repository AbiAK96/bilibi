# Audit screens

`/audit/actions` and the dashboard's Recent Activity panel, ported from
Bilimbi `Audit.Web` (`ActionsLive`, `DashboardActivityPanel`). The evidence,
its capture and its reads live in `base/audit`; this module only presents them.

Why a module of its own: Authz records its grants through Audit
(`base/authz` depends on `base/audit`) and a screen needs Authz and the
Dashboard, which also needs Authz. A screen inside `base/audit` would close that
cycle. Bilimbi breaks it with an application-environment callback; here the
screens sit above both. The audit capabilities (`admin.audit.log.list`,
`.manage`, `.view`), the `auditor` and `system_viewer` role grants and the
"Audit Log" menu branch belong here for the same reason.

## Actions (`/audit/actions`)

Newest first. Search (event, URL, actor role, trace, actor type, payload text),
event family (HTTP, Authentication, Console, SQL console, Queue, Domain),
result (Failures, Retained), diagnostic traffic (hidden by default; failures
still show) and actor type narrow the list in SQL; the URL keeps every filter,
the sort and `page_size`, so Back and reload restore the view. The table is the
shared FlexTable over `ListColumns` (reorder, remove, row height). Bilimbi's
screen has no period filter, so there is none here.

`admin.audit.log.list` guards the route and every live event.
`admin.audit.log.manage` decides whether the bookmark control is drawn, and the
write asks it again: a grant removed after the page opened flashes a refusal
instead of changing the row. Retaining flips a bookmark in
`base_audit_action_metadata`; the recorded `is_retained` bit is evidence and
never changes. Each flip is itself an audited write
(`audit.action.retention`, mutation kind `audit_action_retention`).

## Recent Activity (dashboard)

`bilibi_dashboard` entry `base-dashboard-recent-audit` (medium, order 30, offered
to holders of `admin.audit.log.list`, refreshed every two minutes) drawn by the
`audit.dashboard-activity` embed: the five latest mutations in the tenant. The
embed is `SignedIn` so a card already on the page survives a revoked grant and
shows an em dash without reading. The open control goes to `/audit/actions`;
P13 repoints it to the Data Mutations list, as Bilimbi has it.

## Running it

`examples/actions-fixture.rs` inserts a fixed 64-action ledger (every actor kind
and event family, each result, diagnostic traffic, one bookmark) into a
disposable tenant and prints what each filter returns; see its header. Then open
`/audit/actions` as an account holding the capability (the demo `admin`).
