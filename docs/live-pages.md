# Module live pages and panels

The shared contracts are in `apps/base/ui/src/page.rs`; socket dispatch and guards
are in `apps/web/src/live.rs`. Company implements the contracts in
`apps/core/company/src/web.rs`. Base UI keeps the context/error types generic so
it does not import Security or Core.

Contribute the same `PageService<Scope, Error>` used by HTTP. Implement
`Page::mount` to return a fresh `LiveState` for each socket mount; enable
`PageBody.live` only for a page with a mount adapter. Store typed business state
in that owner, decode a bounded module event enum there, and render escaped HTML
through its templates. The default mount adapter declines live operation.

Every update receives a freshly guarded scope and the server-owned credential.
The owner still authorizes record reads and mutations through its service APIs;
a route capability or a parent page is not a record/subject proof. State should
change only after validation and successful reads/writes. A transient invalid
intent or edit conflict preserves the mounted state. A revoked session, denied
parent route, missing parent record or unavailable parent data clears the page
and closes the socket.

## Transport

`/live` validates cookie and Origin and proves the signed-in session. It does not
require a Company capability. The page is selected by the discovered GET route,
using the same literal/parameter precedence as HTTP, and its framing/access
requirements are checked before mount. Paths are local absolute paths, without
query or fragment; query values travel separately. Percent-encoded path segments
are decoded once and encoded path delimiters, control bytes and invalid UTF-8
are refused. Public HTTP pages remain independent of this authenticated socket.

Client intents use these shapes (an optional unsigned `request_id` is echoed):

```json
{"type":"mount","path":"/companies","params":{"q":"Company"},"request_id":1}
{"type":"navigate","path":"/companies/1","params":{},"request_id":2}
{"type":"params","params":{"page":"2"},"request_id":3}
{"type":"event","target":"page","payload":{"type":"sort","field":"name"},"request_id":4}
{"type":"event","target":"counter-a","payload":{"type":"increment"},"request_id":5}
```

Mount/navigation creates new state and drops old state/subscriptions. Params
updates the current owner. `page` targets its state; any other module target must
be a mounted server-declared panel ID. The temporary `shell` target owns only the
existing notification-read action, outside framed pages. Client payloads cannot
choose an embed, operation, service, tenant, account or record binding.

Responses are `page` (complete `#app`, canonical URL, notice and row count),
`fragment` (one panel's complete root element with its existing ID), or `error`
(message, target and clear flag). Accepted user events echo the request ID;
background updates use null. A rejected intent with a valid request ID also
acknowledges that ID. Canonical query values are encoded by Web, never supplied
as a client URL. User navigation/query changes push history when the URL changes;
mount and background refresh replace it. Browser Back remounts the historical URL.

The browser adapter supports `data-target` on event buttons/forms and these
DOM events:

```js
document.dispatchEvent(new CustomEvent('bilibi:event', {
  detail: {target: 'counter-a', payload: {type: 'increment'}}
}));
document.dispatchEvent(new CustomEvent('bilibi:navigate', {
  detail: {path: '/companies/1', params: {}}
}));
```

The socket processes events sequentially with a 16 KiB message/frame limit.
F6 owns the full browser event queue, keyed DOM reconciliation, hook lifecycle,
reconnect and focus/scroll/iframe retention; the current browser still permits
only one outstanding intent. Panel fragments already target independent DOM
roots; page fragments retain the existing whole-shell adapter until F6.

## Server-declared panels and background updates

`LiveState::panels` returns up to 32 unique `PanelSpec`s: stable HTML element ID,
contributed embed ID and owner-supplied path/query arguments. Live embeds use a
`PageService<Scope, Error>` service and implement mount; their render body must
contain the root element named by the panel ID. The owner must validate the
embed's subject arguments on every update. The legacy notification embed remains
an owner service declaration until C6 supplies its page/panel adapter.

Each instance owns separate state, even for the same embed/service. An unchanged
spec preserves that state across parent refresh; a changed or removed spec drops
it. Parent-route access and panel access are rechecked before panel events and
background callbacks. A panel refusal removes its state and sensitive HTML while
retaining an empty DOM anchor, leaving the parent and siblings usable. A restored
permission can mount it again during a parent refresh.

Return `Subscription`s from the mounted state's `subscriptions` method. Publish
`Change` only after a business transaction commits, with its tenant and optional
recipient account. Web forwards wakes through a bounded 64-entry channel,
rechecks scope/access, discards other tenants/recipients and stale mount
generations, then refreshes only the target. Broadcast lag causes a guarded
refresh rather than treating a lost message as current data. Dropping the mount
aborts subscription tasks and releases receivers. Modules that own additional
tasks/resources must release them when their state drops.

The existing recipient-scoped notification channel and 15-second fallback also
revalidate the page and child panels. Neither notification, subscription nor
timer refresh records session activity. Only accepted user navigation/events do.

## Running reference specimen

Use an isolated migrated/demo-seeded database as described in
`docs/runtime-parity.md`, then run:

```sh
DATABASE_URL='<isolated database URL>' \
APP_ORIGIN='http://127.0.0.1:4315' BIND_ADDR='127.0.0.1:4315' \
cargo run -p bilibi-web --example live-reference
```

Log in with a demo account, visit `/live-reference`, increment either panel and
announce from the page. Each counter retains independent state and subscriptions;
Company pages are served by the same app. This is a manual specimen command,
not a test suite or a product route. Its two panels require `notifications.view`;
the parent requires only sign-in, allowing independent revoke/restore and
Company-denied socket demonstrations. The normal app does not register this route.

F5 runtime evidence lives outside the repository in the task evidence directory.
Company list/filter/Back/save, targeted panel identity, subscription wakes,
independent permission loss and unchanged background `last_activity` were
observed through Chrome and the disposable database. Bilimbi's running reference
returned HTTP 500 (`ArgumentError at GET /`), so an equal-fixture visual comparison
could not be completed. No visual-parity claim is made by this transport slice;
its Company HTML/templates/styles are unchanged.
