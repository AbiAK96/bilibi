# System pages

`base/system` owns `/system/info`, `/system/localization` and
`/system/menu-inspector`. Each requires its contributed platform capability
**and** the persisted operator identity. The `system_viewer` role contributes
Info and Inspector read capabilities; use the normal explicit
`authz-reconcile` command to reconcile an existing installation. No grant or
operator marker is installed by visiting a page.

Pages compose the shared UI, list state, form and FlexTable components and are
eligible for chromeless workspace frames. The host guards HTTP, socket mount,
events, URL changes and refresh; owners also reauthorize calls to their page
services. No unit tests or test-only infrastructure are included.

System Info reads facts once per HTTP navigation, matching Bilimbi's static
snapshot. A mounted page retains those facts while the live transport keeps
shell panels and authorization active; background refresh does not re-probe.
Rust replaces Elixir/OTP/ERTS with the actual compiler, Tokio worker
and live process facts from Perf. The Database card exercises a fresh bounded
query, exposes no connection string or credentials, and distinguishes an
unreachable database from a missing probe. Applications lists the installed
registry modules with their workspace version, rather than pretending Rust
has loaded OTP applications. The environment is explicitly the build profile,
debug means debug assertions, and timezone describes UTC timestamp storage.
`APP_ORIGIN` follows the Web command's configured origin and default.

Unavailable probes stay visible. Disk capacity has no installed portable
probe. Queue supplies job states but no executor-heartbeat health boundary;
its health is unavailable rather than inferring an active executor from an
empty or reachable queue. Perf combines recorder and history-store health and
reports disabled collection explicitly. Settings has no value cache; its row
says that values are read directly from PostgreSQL.

Language & Region reads `Locales::resolve_global`, so an account locale cannot
masquerade as the installation choice. Options come only from `SUPPORTED`.
Every save reauthorizes inside the audited mutation transaction and calls
`Locales::put_global`: this atomically records manual provenance and removes
any inferred-country fact. Unsupported choices are refused visibly. Bootstrap
provenance remains read-only and no Core address tables are queried. English
product copy is the only installed copy; regional support is not translation.

Menu Inspector lists the complete validated Menu catalog and module
provenance. Allowed is the item's own capability rule (including any-of and
direct denials), not whether an ancestor happens to display it. Served checks
the actual contributed GET route patterns. Source/search filters and bounded
25/50/100/300 pagination survive Back and reload through `ListState`; shared
`ListColumns` owns column add/remove/move and zoom. No page-owned table or
browser event controller duplicates those components.

Runtime and screenshot evidence for slice O5 is recorded outside the checkout
under the task's `data/bilibi-o5/` directory. Follow `docs/runtime-parity.md`
from the repository root for disposable database and browser procedures.
