# Discovered page runtime checks

Run these cases against an isolated database as described in
[runtime-parity.md](runtime-parity.md); no unit-test suite is involved.

Register a small reference module through the normal inventory contracts, with
its `PageService` rendering a recognizable operation label. Keep reference
contributions in a separate fixture executable so production has no fixture routes.

| Case | Exercise | Expected observation |
| --- | --- | --- |
| Literal overlap | Contribute `/companies/reference` beside Company's `/companies/{id}` | Literal operation renders; `/companies/1` still renders Company detail. |
| Ambiguous overlap | Contribute GET `/dispatch/{one}/end` and `/dispatch/start/{two}` | Startup fails with both paths in the contribution error. |
| Host shadow | Contribute `/live` | Startup fails before binding the server. |
| Wrong service | Reference an existing service with a different concrete type | Startup fails with missing/wrong service type. |
| Signed in | Visit a SignedIn page anonymously, then with a valid session | Anonymous GET redirects to login; authenticated GET renders. |
| Capability | Visit a Capability page with a direct denial | 403; the page owner is not called. |
| Operator | Visit OperatorCapability before and after adding the explicit operator identity to this disposable tenant | 403 before; allowed after only with the capability. Other tenants and explicit capability denials remain forbidden. |
| Unsafe method | POST with missing Origin, then missing CSRF, then both valid | 403, 403, then owner dispatch. |
| Shell isolation | Deny the capability a shell or dashboard embed requires (`companies.view` on `dashboard-reference`) and visit an allowed page | Page still renders; the denied embed reads no data. |
| Framing | Navigate an iframe to Companies | `#app-shell[data-framed=true] > main#app-content`, without menu, header or bell. One vertical scroller. |
| Frame refusal | Navigate an iframe to a route declaring `framed: false` | 403 before owner dispatch. A top-level query `?framed=true` does not hide the shell. |
| Live framing | Sort and wait for refresh inside the Company frame | Framed markup stays chromeless; typed URL state changes normally. |
| Forbidden mount | Connect an authorized socket, revoke companies.view, then send its mount | `Access denied`, `clear: true`, socket closes; no rows arrive. |

## Download routes

`apps/web/examples/download-reference.rs` serves the cases below on an isolated
database with the walkthrough fixture (`manager@example.test` holds
`reference.exports.view` through the fixture manager role; `viewer@example.test`
does not).

| Case | Exercise | Expected observation |
| --- | --- | --- |
| Attachment | Manager GETs `/reference/exports/1/csv` | 200, `text/csv; charset=utf-8`, `content-disposition: attachment; filename="…"; filename*=UTF-8''…`, `cache-control: private, no-store`, `nosniff`, `default-src 'none'; sandbox; frame-ancestors 'self'`. |
| Inline | Manager GETs `/reference/exports/1/print` | 200 `text/html` shown in the tab; the page's inline script is blocked by `default-src 'none'`. |
| Encoded name | GET return 3, whose name carries `é`, an em dash and quotes | ASCII fallback replaces them; `filename*` percent-encodes the UTF-8 name. |
| Anonymous | GET a download with no session | 303 to sign in; the owner is not called. |
| No capability | Viewer GETs a download | 403; the owner is not called. |
| Revoked grant | Manager downloads, then a direct deny (or the role's unassignment) is written, then the same session GETs again | 200, then 403 without a new sign-in and with no owner line in the server log; lifting the deny restores 200. |
| Missing file | Manager GETs `/reference/exports/9/csv` | Bare 404, no missing-record redirect. |
| Overlap | Contribute a download at a page's path, or at `/live` | Startup fails naming both paths. |

The D4 evidence run on 2026-10-16 (database `bilibi_d4_runtime`, origin
`http://localhost:4344`) recorded every case above with `curl` and the browser;
the probe script, its output, the server log and the screenshots are in
`/home/kiat/repo/kunchenguid/firstmate/data/bilibi-dom-d4/evidence/`.

The F4 evidence run on 2026-10-14 used the existing Companies fixture, a separate
`extensions/dispatch_reference` runtime executable, and the disposable database
`bilibi_f4_runtime`. Compiler, Clippy, discovery and format checks passed.
The reference executable demonstrated valid literal overlap, ambiguous overlap
and host shadow failure, access/Origin/CSRF refusals, explicit operator proof,
and framing eligibility. Browser evidence used chrome-devtools-axi at 1440×1000.
After a frame sort/refresh, document/body height stayed 1000 while the single
`#app-content` scroller contained 1407 pixels; menu and bell remained absent.
A connected socket refused a post-revocation mount and closed with no page data.

Artifacts and the exact fixture driver are in the dispatch's external evidence
folder `/home/kiat/repo/kunchenguid/firstmate/data/bilibi-f4/`. Do not add tokens,
cookies or passwords to evidence. Framed DOM/classes were copied from Bilimbi's
`apps/base/ui/lib/ui/layouts.ex`, and request detection follows
`apps/web/lib/bilimbi_web/framed_render.ex`. This run captures Bilibi screenshots;
it does not establish full Company visual parity with a seeded Bilimbi instance.
Shared components, exact shell, tiling and full parity remain their assigned
U/P/T/Z slices.
