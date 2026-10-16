# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Correct entries that work proves wrong; add new ones only by deliberate maintainer choice, never as routine task output.

## Mounting Domain containers

- A Domain is its own Git repository, `BelimbingApp/bilibi-dom-<id>`, mounted as a real nested checkout at `apps/domains/<container>`, where `<container>` is the `<id>` in snake_case (`bilibi-dom-supply-chain` mounts at `apps/domains/supply_chain`). It is not a submodule and never a symlink; Askama and path dependencies resolve from the real mount location. Mount with `git clone <repository URL or local clone> apps/domains/<container>`, then `python3 scripts/discover-modules.py` and any Cargo command (which adds the mounted crates to `Cargo.lock`).
- The container root holds `bilibi.container.toml` (`id = "<container>"`, `layer = "domain"`) and one folder per module, each a crate named `bilibi-<container>-<module>` (underscores become hyphens) whose manifest inherits the workspace (`version.workspace = true`, `x.workspace = true` for Platform crates and `rust_decimal`). No `Cargo.toml`, `[workspace]` or `Cargo.lock` at the container root; `fixtures/` and dot folders are never discovered. `scripts/discover-modules.py` refuses every other shape, so read its refusal rather than working around it.
- Mounted crates are ordinary Platform workspace members, so `cargo clippy --workspace --all-targets -- -D warnings` lints them; an out-of-tree composition workspace would not.
- Never commit `apps/domains/Cargo.toml`, `apps/domains/src/lib.rs` or `Cargo.lock` from a mounted checkout: the Platform's committed forms are the unmounted ones, and `--locked` does not notice extra mounted packages. Commit and push only inside the container's own repository, and make a Platform change with nothing mounted. `python3 scripts/discover-modules.py --check` refuses a staged or committed mounted composition or a `Cargo.lock` path package outside the Platform.
- To unmount, move the container out of `apps/domains`, run `python3 scripts/discover-modules.py`, and restore `Cargo.lock` (`git checkout -- Cargo.lock`, or any Cargo command without `--locked` prunes it); `git status` is then clean.
- A Domain must not turn on features of shared crates (for example `serde_json/preserve_order`): Cargo unifies features across the workspace, so the change would reach every Platform crate.
