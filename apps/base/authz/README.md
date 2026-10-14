# Authorization boundary

Use `authorize_key` with a fresh Tenancy scope for a capability decision. Decisions query current roles/direct grants, persist an allow/deny ledger row and propagate logging failures. A direct deny wins over direct or role grants, including `grant_all`. The `Capability` enum and `restrictions` function preserve the Companies demo while P6 supplies contributed vocabulary and richer role policy.

Use `restrictions_for(client, scope, table)` for any-role field restrictions. Table names are bound values, never SQL identifiers; a granting administrator role does not remove another role's restrictions. The historical storage constraint currently limits configured restrictions to Companies; P6 owns its additive schema expansion.

Use `authorize_operator` when an operation requires both explicit operator identity and a capability. Neither a numeric tenant shortcut nor a business party relationship substitutes for either proof. `base/security` is a compatibility facade; new owners depend directly on Tenancy, Session and Authz.
