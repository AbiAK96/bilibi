# Settings editor (P3)

`/system/settings` is the operator page for contributed settings (Bilimbi
`Settings.Web.GroupLive` + `Settings.Form` + the host's `SecretReveal`). It is
its own module because it needs Authz, and Authz depends on `base/settings`:
storage stays below Authz, the screen sits above it.

## Fields

A field is every `base/settings.definitions` contribution with `editor`
metadata, a concrete key (no wildcard) and the global scope, sorted by key, in
one group as Bilimbi's single `operator` page shows them. Declaring such a
setting is all a module does to appear here. A field whose `editor.capability`
the actor does not hold right now is withheld: it is not rendered and a forged
submission naming it is ignored. When every field is withheld the page names
the missing capabilities instead of claiming nothing was contributed.

Values are read with `Settings::resolve_masked`, so an encrypted value is never
decrypted to draw the page; a stored secret shows the keep-current mask.
Without a valid `BILIBI_SETTINGS_KEY` an encrypted field is read-only (it can
still be cleared by Restore Defaults, which needs no key).

## Writes

Route access is `OperatorCapability(base.settings.global.manage)`. Every Save
and Restore re-asks `authorize_operator` (logged) and rebuilds the field plan
from live grants before acting; a refusal flashes and redraws, it never clears
the page. A Save is planned in full, then applied in one `MutationTransaction`
(shared mutation audit): a blank clears this scope's override, the mask keeps a
secret, and a value equal to what the field already shows is not written.
That last rule is a deliberate difference from Bilimbi, whose Save pins every
submitted value, including inherited defaults and every checkbox, as an
override. Messages say what happened ("1 setting updated, 1 override
cleared."). Restore confirms through the shared dialog and deletes only the
overrides the dialog counted that are still overridden.

## Stored-secret reveal

"Show stored value" appears only for an actor holding
`base.settings.secret.view` by name (`authz::explicitly_granted`; `grant_all`
does not count) when the key is configured. The actor re-enters their own
password (`tenancy::Authentication::confirm_password`, bound by Core User);
five failures a minute per account are throttled in-process. Every attempt,
including a forged request, records a retained `settings.secret.reveal` audit
action without the value, and a reveal whose audit row cannot be written shows
nothing. The value reaches the browser only as a `LiveBody.events` push to the
`SecretStored` hook (`bilibi:secret:reveal`), which re-masks it after the
definition's `reveal_duration_ms`.

## Company scope

Accounts with `base.settings.company.manage` see the scope picker, but it holds
Global only and the page says no company is available: Core binds company
targets, archived read-only state and company groups in port slice K15. Others
see Bilimbi's "You do not have permission to manage company settings."

## Manual walkthrough

On a disposable migrated and seeded database, appoint tenant 1 as operator
(`INSERT INTO platform_operator(singleton,tenant_id) VALUES(true,1)`), set
`BILIBI_SETTINGS_KEY` to 64 hex characters, run the app and sign in as a
tenant 1 account with `base.settings.global.manage` (an admin role with
`grant_all`). Then:

1. Open **Administration → System → Settings**. Each card says "Set here" or
   "Inherited from the default".
2. Change a number and Save: one setting updated. Blank it and Save: one
   override cleared. Type `abc` into a number: "<Label>: must be a whole
   number", nothing stored.
3. Save two changes, press Restore Defaults: the dialog counts two overrides;
   Restore clears them.
4. Grant the account `base.settings.secret.view` directly and store an
   encrypted editable setting (the walkthrough in `apps/web/examples` or any
   module's encrypted definition). "Show stored value" asks for the password;
   a wrong one is refused, the right one shows the value briefly, and
   `base_audit_actions` holds one `settings.secret.reveal` row per attempt.
5. Deny one field's capability (`principal_capabilities ... is_allowed=false`)
   and reload: that card is gone; deny all and the page names them.
