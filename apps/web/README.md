# Discovered pages

Register HTTP pages with `Contribution::new(ROUTES, ...)` from
`bilibi_module_registry::contributions`. The named service must resolve to
`bilibi_ui::page::PageService<bilibi_tenancy::Scope, bilibi_database::Error>`,
wrapping an owner implementation of `Page<Scope, Error>`. The generic UI contract
imports no identity/database crate, so Tenancy can compose pages without a cycle.
Company's adapter in `apps/core/company/src/web.rs` is the current example.
Do not add module page paths or business dispatch switches to the host router.

`src/routes.rs` binds every discovered route at boot. Missing/wrong service
adapters, malformed paths, host-path shadows, renamed shared parameters and
ambiguous intersecting patterns fail startup. Literal children such as
`/companies/legal-entity-types` take precedence over `/companies/{id}`. Parameter
names must agree at shared nodes across HTTP methods. Paths use whole-segment
`{name}` parameters; catchalls and optional/embedded parameters are unsupported.
The registry also rejects identical patterns with different parameter names.

Access is checked before the owner renders: Public, SignedIn, Capability(key),
Operator, or OperatorCapability(key). Operator requires the persisted Tenancy
proof, never a tenant-ID convention or an admin grant. OperatorCapability
requires both proof and capability; direct denials still win. Browser GETs
without a valid session redirect to login. Authenticated refusals return 403.
Other methods also require the configured Origin and, for an authenticated
scope, `X-CSRF-Token`. Owners must reauthorize mutations inside their transaction.
The adapter receives the operation, proven optional scope, path/query values,
bounded request body and presentation flag. HTML escaping belongs to the owner.

Framing comes from `Sec-Fetch-Dest: iframe` (or `frame`) on each HTTP navigation,
never a cookie, account setting or query parameter. A route with `framed: false`
returns 403 inside a frame. Allowed pages use Base UI's `Framed` template, copied
from Bilimbi's chromeless layout marks/classes; no shell, menu or bell is loaded.
CSP permits same-origin frames. The frame flag grants no authority. The legacy
socket carries a cosmetic framing hint so mounts and refreshes keep that layout;
it rechecks route access and framing eligibility on mount/events/refreshes.

`PageBody.live` enables the current browser socket adapter. Until F5 introduces
module-owned page state/events, only the existing Company adapter supports live
updates. Other HTTP adapters should return `live: false`. F5 replaces the
Company-specific socket enum, row snapshot, upgrade check and refresh dispatch;
F6 owns queued events and retained fragment/iframe lifecycle. The HTTP adapter
and `PageService` are the seam those slices can reuse.
