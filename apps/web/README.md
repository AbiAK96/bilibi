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

`PageBody.live` enables the browser socket adapter for owners implementing
`Page::mount` and `LiveState`. See `docs/live-pages.md` for module-owned state,
targeted panels, URL changes and guarded subscriptions. HTTP-only adapters should
return `live: false`. F6 owns queued events and retained fragment/iframe lifecycle.

# Agent edge (`src/api.rs`)

The host owns the machine endpoints; discovered pages cannot claim them
(`api::reserved`: `/api/v1/…`, `/.well-known/…`, `/oauth/device_authorization`,
`/oauth/token`, `/mcp`).

- Discovery: `/.well-known/oauth-authorization-server`,
  `/.well-known/oauth-protected-resource[/mcp]` and `/.well-known/bilibi`, all
  built from `APP_ORIGIN`, never the Host header.
- `POST /oauth/device_authorization` and `POST /oauth/token` take urlencoded or
  JSON fields and delegate every decision to `core/agent_access.grants`
  (device, refresh and PKCE code grants). Errors are `400 {"error":code}`;
  storage failure is `503 temporarily_unavailable`. Each endpoint has its own
  per-sender bucket from `agent_access.oauth_requests_per_minute`. The sender
  is the peer address, or the first non-loopback `X-Forwarded-For` hop when the
  peer is a loopback proxy. `main.rs` serves with `ConnectInfo` for this.
- `/api/v1`: `GET search` (`q`, `limit` 1–50, `kind` read|write,
  `include_unavailable`), `GET operations/{key}`, `GET guides/{key}`,
  `POST operations/{key}/call` (`{"input":{…}}`), `POST plans`
  (`{"operations":[…]}`) and `POST plans/commit` (`{"plan_code","operations"}`),
  with Bilimbi's status codes and `{"error":…}` bodies. Unknown paths and
  methods also answer JSON. Bodies are capped at 2 MiB.

Every `/api/v1` request proves the actor live: `AgentAccess::authenticate` maps
the bearer to claims, then `AgentAccess::seal` reproves the grant, the account
and its party facets and seals a fresh scope with the agent facet. Plans use
the `Grant` `PlanAuthority`, which runs `AgentAccess::prove` again with the
grant row locked inside the plan transaction. A revocation that commits
mid-request therefore refuses the commit with 401. A refused bearer gets
`401 invalid_token` with `WWW-Authenticate: Bearer`. API tokens carry no
resource, so an MCP-bound token is refused here. Use `authenticate` and
`Grant`, not a second credential check, when adding `/mcp` (E6).

The `record` middleware writes one incoming Integration exchange per request,
refusals included. It records method, path, the operation key (or
`agent_api.plan`/`agent_api.commit`), status, duration, connection id and the
self-reported `X-Bilibi-Agent-Id`/`X-Bilibi-Run-Id` labels; bodies, queries,
headers and tokens are never recorded. Sealed requests record a tenant row;
everything else records a system row. The trace in the `x-bilibi-trace-id`
response header is the exchange's correlation ID and the audit trace of any
mutation that request committed.
