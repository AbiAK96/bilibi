# DateTime display and period contract

`base/datetime` is a policy boundary, not date arithmetic: storage stays UTC and
business modules keep their own deadline semantics. It owns two Settings
definitions and the request-scoped display context presentation renders with.

- `localization.timezone` — tenant/company scoped IANA identifier, validated by
  `valid_timezone` against the bundled `jiff` tzdb (links such as
  `Asia/Kuala_Lumpur` included, `timezones()` lists them). Core Company owns the
  editor and writes through Settings under its own capability; Base never
  queries Core. Editor metadata is attached by P3.
- `ui.timezone.mode` — the actor's own `company | local | utc`, default `company`.
- `DisplayPolicy::display(client, actor, company, locale)` returns a
  `DisplayContext { mode, timezone, locale }`. The locale is the one the caller
  already resolved; pass `bilibi_locale::Resolved::locale`. `company` is a
  request: Settings reproves it with the Company provider on the same
  connection, so an ineligible company refuses. A stored zone that no longer
  converts falls back to the default instead of breaking pages.
  `DisplayContext::anonymous()` is the pre-sign-in `local` context.
- `DisplayContext::render` returns `Rendered` carrying both server-decidable
  texts (`company`, `utc`), the mode's own `text` and an RFC 3339 `iso` value.
  The shared timestamp template must emit both (`data-text-company`,
  `data-text-utc`) so the browser follows a mode change by copying a server
  string; `Local` is the only mode the browser formats. Dates are zone-free
  (`render_date`). Format is `dd/mm/yyyy, HH:MM` with a zone abbreviation, as in
  Bilimbi. Do not format an instant by hand in a template.
- `Period` parses `period/from/to`, with inclusive days and the Last 30 days
  default. `bounds(zone)` converts to a half-open UTC range under the zone's
  real rules (23/25-hour days; a midnight gap starts at the first valid
  instant, a fold at the earlier). `Clock::resolve` picks the zone for "today"
  from the mode; the browser zone is accepted only in `Local`, validated, and
  otherwise UTC.

Run `BILIBI_DISPOSABLE_P4_DEMO=yes cargo run -p bilibi-web --example
p4-locale-datetime` against a migrated, seeded disposable database (it appoints
tenant 1 operator and resets only the keys it uses).
