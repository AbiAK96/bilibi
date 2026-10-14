# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Correct entries that work proves wrong; add new ones only by deliberate maintainer choice, never as routine task output.

## Shared UI primitives

- Use the macros in `templates/components.html` (`{% import "components.html" as ui %}`) for page, header, section heading, card, facts, stat strip, button, icon button, create button, back link, action link, badge and icon, not hand-written markup: that file is the one DOM source, mirroring Bilimbi `apps/base/ui/lib/ui/components.ex` at the pinned snapshot, so a screen that copies classes drifts from parity the next time the shared file changes. A crate outside `base/ui` reaches the same macros through the typed models in `src/components.rs` (`Button`, `Card`, `Facts`, ...), which render through the same macros.
- Name variants through the enums in `src/components.rs` (`ButtonVariant`, `BadgeKind`, `IconContext`, `IconKind`, `Chrome`, `PageVariant`, `StatKind`), not class strings; the class decisions live in their methods and in `card_flat`, `card_pads` and `stat_strip_columns`.
- Pass secondary slots (`actions`, `title_actions`) as an `Html` fragment rendered by another component, and keep `|safe` inside the shared macros only; see `src/html.rs`. Request data goes through `Html::text` or a template, never `Html::raw`.
- Askama refuses a macro nested in its own body, so a card inside a card renders the inner one through the typed `Card` model; `card_flat`/`card_pads` resolve the `p-0` and padding conventions, so do not append padding classes expecting the default to win.
- Name icons through `src/icons.rs` action names (`"edit"`, `"manage"`) or `hero-*`; an unregistered name panics on purpose, and names from stored data pass `icons::renderable` first.
- `/assets/ui-components.html` (`src/specimen.rs`) is the screenshot baseline for every primitive and variant; extend it when a primitive or variant is added, and keep its content mirrored in the Bilimbi reference renderer kept with the task evidence, not in this repo.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
