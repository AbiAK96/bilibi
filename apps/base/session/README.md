# Session guard boundary

Use `base/session.sessions` (`Sessions` in `src/lib.rs`) for durable session authentication. `check(client, token)` rechecks absolute and idle expiry, the live tenant and the Core-owned account verifier without writing session activity. Use it on background timers, subscription callbacks, socket upgrades and service revalidation. `activity(client, token)` performs the same checks and renews only an eligible, still-existing session row. Use it only after accepting authenticated HTTP navigation or a user event. Invalid events and failed authorization do not renew activity.

Renewals are throttled, and activity takes a row lock while deciding whether renewal is due. A revocation between initial lookup and renewal is refused. Background reads do not lock or update session rows. Checks use the caller's transaction when supplied. Expiry is enforced during authentication and never depends on a cleanup job.

`Policy::new` validates idle lifetime and touch interval (the latter at most half the lifetime). Defaults match Bilimbi: 120 minutes idle and five minutes between renewals. The existing 24-hour absolute expiry remains. P2/P15 bind configurable operator settings and administration/pruning; these interfaces do not introduce those screens.

The Web adapter calls `activity` after successful page loading or accepted events. The 15-second refresh timer and bell subscription only call `check` via the compatibility facade. The Core account provider is re-run for every check, so deleted or suspended accounts cannot keep using an existing session.
