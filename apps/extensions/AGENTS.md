# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Correct entries that work proves wrong; add new ones only by deliberate maintainer choice, never as routine task output.

## Mounting Extension containers

- An Extension is its own Git repository, `BelimbingApp/bilibi-ext-<id>` (or a company's own repository), mounted as a real nested checkout at `apps/extensions/<container>`, where `<container>` is the `<id>` in snake_case. Mount with `git clone <repository URL or local clone> apps/extensions/<container>`, then `python3 scripts/discover-modules.py` and any Cargo command.
- Its root `bilibi.container.toml` declares `id = "<container>"` and `layer = "extension"`; modules follow the Domain container rules in `apps/domains/AGENTS.md` (crate `bilibi-<container>-<module>`, no root `Cargo.toml`, `[workspace]` or `Cargo.lock`, no symlinks, no features of shared crates). A container ID may be mounted under only one of `apps/domains` and `apps/extensions`.
- Never commit `apps/extensions/Cargo.toml`, `apps/extensions/src/lib.rs` or `Cargo.lock` from a mounted checkout; commit only inside the container's repository and make Platform changes with nothing mounted. `python3 scripts/discover-modules.py --check` refuses a staged or committed mounted composition.
