# Dashboard agent notes

- Contribute an entry with `bilibi_dashboard::contribute!` plus an `Embed` whose context is `EmbedContext::Dashboard`, not a card or page of your own on `/dashboard`: the page mounts each entry as an independent server-declared panel, so permission loss, refresh and drag order work per entry; `README.md` is the contract and `apps/web/examples/dashboard-reference.rs` a runnable specimen.
- A panel renders one root element whose id is `PanelArgs::id()` and reads through `PanelCache`, not on every `update`: the host refreshes every panel on every parent update and on its 15 s tick, so an uncached panel turns each Customize click into a read.
- Never rename an entry id or rewrite the `ui.dashboard.*` keys: ids are stored in each account's arrangement.
- The page keeps each panel in a keyed `data-live-ignore` slot (`slot-<id>`) inside a keyed wrapper; drop the keys or the ignore and a page patch blanks the panels until the next fragment.
