# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Correct entries that work proves wrong; add new ones only by deliberate maintainer choice, never as routine task output.

## Shared UI primitives

- Use the macros in `templates/components.html` (`{% import "components.html" as ui %}`) for page, header, section heading, card, facts, stat strip, button, icon button, create button, back link, action link, badge and icon, not hand-written markup: that file is the one DOM source, mirroring Bilimbi `apps/base/ui/lib/ui/components.ex` at the pinned snapshot, so a screen that copies classes drifts from parity the next time the shared file changes. A crate outside `base/ui` reaches the same macros through the typed models in `src/components.rs` (`Button`, `Card`, `Facts`, ...), which render through the same macros.
- Name variants through the enums in `src/components.rs` (`ButtonVariant`, `BadgeKind`, `IconContext`, `IconKind`, `Chrome`, `PageVariant`, `StatKind`), not class strings; the class decisions live in their methods and in `card_flat`, `card_pads` and `stat_strip_columns`.
- Pass secondary slots (`actions`, `title_actions`) as an `Html` fragment rendered by another component, and keep `|safe` inside the shared macros only; see `src/html.rs`. Request data goes through `Html::text` or a template, never `Html::raw`.
- Askama refuses a macro nested in its own body, so a card inside a card renders the inner one through the typed `Card` model; `card_flat`/`card_pads` resolve the `p-0` and padding conventions, so do not append padding classes expecting the default to win.
- Name icons through `src/icons.rs` action names (`"edit"`, `"manage"`) or `hero-*`; an unregistered name panics on purpose, and names from stored data pass `icons::renderable` first.
- Use the macros in `templates/forms.html` (`{% import "forms.html" as form %}`) for every form control (input of any type, select, textarea, checkbox, password with reveal, secret and stored-secret input, code input, combobox, multi-select, radio group, restricted value and restricted field), not hand-written `<input>` markup: that file mirrors Bilimbi `apps/base/ui/lib/ui/components/forms.ex`, and each macro takes one typed model from `src/forms.rs` (`Field`, `SecretInput`, `CodeInput`, `Combobox`, `MultiSelect`, `RadioGroup`, `Restricted`, `RestrictedField`) built with its chained setters. Field classes, described-by wiring, invalid state and the keep-current mask are decided in those models, so a page passes errors, hints and `readonly`/`disabled` and never composes control classes itself; a stored secret passes `stored()` and the model discards the value.
- Browser behaviour of a control hangs off `data-hook` in `assets/hooks.js` (served at `/assets/hooks.js`), which mounts on insertion and destroys on removal by itself: a page adapter forwards a hook's `hook:event` CustomEvent to the server and dispatches a server push as `window` event `bilibi:<name>`; it never binds listeners to a control's parts directly. A control that reports to the server carries `data-event` (plus `data-name`/`data-option`/`data-key`), the counterpart of Bilimbi's `phx-click`.
- `/assets/ui-components.html` (`src/specimen.rs`) is the screenshot baseline for every primitive and variant; extend it when a primitive or variant is added, and keep its content mirrored in the Bilimbi reference renderer kept with the task evidence, not in this repo.

## Feedback, dialogs and tabs

- Use `templates/feedback.html` (flash, `flash_group`, `connection_banners`, `alert`, `panel_notice`), `templates/dialog.html` (`modal`, `confirm_dialog`) and `templates/tabs.html` (`tabs`), or their typed models in `src/feedback.rs`, not hand-written `<dialog>`, banner or tab markup: the DOM mirrors Bilimbi `components.ex`, and `assets/css/app.css` selectors (`body:has(dialog:modal[data-owns-flash])`, the `#connection-*` ids) depend on it.
- Never use `window.confirm` or a `data-confirm` attribute; an irreversible action renders `ConfirmDialog` while its request is pending and stops rendering it whatever the outcome. The server owns whether a dialog exists, so Escape and Cancel only raise the cancel event.
- Browser behaviour is `assets/ui.js`: a component names its controller with `data-hook`, and a controller reports intent with a bubbling `bilibi:event` CustomEvent (`detail.event` plus values), which the transport turns into a server event. The transport reports the socket with `bilibi:disconnected` (`detail.kind` `client`/`server`) and `bilibi:connected` on `window`; new controllers register with `BilibiUI.registerHook`. Do not ship Phoenix hooks (`pushEvent`, `mounted`-only lifecycles) unchanged.
- Pick a message kind by what it does: a completed write is `Success` (times out), a refusal or failure is `Error`, `Info` informs without confirming a write; only success carries auto-dismiss.
- `/assets/ui-feedback.html` (`src/feedback_specimen.rs`) is the screenshot baseline for these; its `static/ui-feedback-demo.js` stands in for the server, so extend both when adding a variant.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
