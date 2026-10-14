# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Correct entries that work proves wrong; add new ones only by deliberate maintainer choice, never as routine task output.

## Media

- Store and serve files through `Media` (`base/media.store`, `README.md`), not hand-rolled `std::fs` writes or your own download handler: it owns the private-disk rules, libmagic type checks, tenant scoping and the signed route, and none of them can be bolted on afterwards.
- Build stream headers with `StreamFile::delivery` (`src/response.rs`); only raster images may render inline, so never set `content-type` from stored metadata yourself.
- A call that writes bytes takes a plain `&mut Client` because files and rows cannot share a rollback; use `register_original`/`register_derivative` inside your own `MutationTransaction` only for bytes you already placed.
- Private attachment bytes must be registered with `NewAsset::private`; that flag alone keeps the generic stream, derivative and delete paths away from them.
- Private files for your records come from a `Subject` contribution (`src/owner.rs`), which makes Media ask your `SubjectOwner` on every access; do not skip it by checking the asset id yourself.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
