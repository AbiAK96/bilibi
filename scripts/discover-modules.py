#!/usr/bin/env python3
"""Generate the four Cargo composition crates from deep-module manifests.

Cargo resolves dependencies before build.rs runs, so composition manifests must
be refreshed before Cargo. Runtime services and SQL are inventory registrations
owned by each module; there is no second runtime module list here.

Base and Core modules sit one level deep (`apps/<layer>/<module>`). Domain and
Extension modules sit two levels deep inside a mounted container checkout
(`apps/<layer>/<container>/<module>`) that declares itself in
`bilibi.container.toml`. The committed composition is always the unmounted one,
so `--check` also refuses a staged or committed mounted composition or
`Cargo.lock`.
"""
import argparse
import re
import subprocess
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parent.parent
LAYERS = ('base', 'core', 'domains', 'extensions')
CONTAINER_LAYERS = {'domains': 'domain', 'extensions': 'extension'}
CONTAINER_FILE = 'bilibi.container.toml'
RESERVED_IDS = {'base', 'core', 'web', 'src'}
SNAKE = re.compile(r'[a-z][a-z0-9]*(_[a-z0-9]+)*')
# Path packages the Platform itself owns besides discovered Base and Core modules.
PLATFORM_MANIFESTS = ('apps/web/Cargo.toml', 'fixtures/*/Cargo.toml')


class Refused(Exception):
    pass


def package_name(manifest):
    return tomllib.loads(manifest.read_text())['package']['name']


def snake_id(value, what, where):
    if not isinstance(value, str) or not SNAKE.fullmatch(value):
        raise Refused(f'{where}: {what} {value!r} must be snake_case ([a-z][a-z0-9_]*)')
    if value in RESERVED_IDS:
        raise Refused(f'{where}: {what} {value!r} is reserved ({", ".join(sorted(RESERVED_IDS))})')
    return value


def container_modules(root, layer):
    """Validate each mounted container of a Domain or Extension root; yield (path, crate)."""
    role = CONTAINER_LAYERS[layer]
    for folder in sorted((root / 'apps' / layer).iterdir()):
        if folder.name.startswith('.') or folder.name == 'src' or not (folder.is_dir() or folder.is_symlink()):
            continue
        where = f'apps/{layer}/{folder.name}'
        if folder.is_symlink():
            raise Refused(f'{where}: a container is mounted as a real checkout, not a symlink')
        for stray in ('Cargo.toml', 'Cargo.lock'):
            if (folder / stray).exists():
                raise Refused(f'{where}: a container has no root {stray}; only its modules are crates')
        descriptor = folder / CONTAINER_FILE
        if not descriptor.is_file():
            raise Refused(f'{where}: missing {CONTAINER_FILE}')
        try:
            declared = tomllib.loads(descriptor.read_text())
        except tomllib.TOMLDecodeError as error:
            raise Refused(f'{where}/{CONTAINER_FILE}: {error}') from None
        unknown = sorted(set(declared) - {'id', 'layer'})
        if unknown:
            raise Refused(f'{where}/{CONTAINER_FILE}: unknown keys {unknown}; expected id and layer')
        container = snake_id(declared.get('id'), 'container id', f'{where}/{CONTAINER_FILE}')
        if container != folder.name:
            raise Refused(f'{where}: {CONTAINER_FILE} declares id {container!r}; mount it at apps/{layer}/{container}')
        if declared.get('layer') != role:
            layer_folder = {v: k for k, v in CONTAINER_LAYERS.items()}.get(declared.get('layer'))
            hint = f'; mount it under apps/{layer_folder}' if layer_folder else ''
            raise Refused(f'{where}: {CONTAINER_FILE} declares layer {declared.get("layer")!r}, '
                          f'but apps/{layer} holds layer {role!r}{hint}')
        for manifest in sorted(folder.glob('*/Cargo.toml')):
            module = manifest.parent
            if module.name.startswith('.') or module.name == 'fixtures':
                continue
            if module.is_symlink():
                raise Refused(f'{where}/{module.name}: a module is a real folder, not a symlink')
            snake_id(module.name, 'module folder', f'{where}/{module.name}')
            name = package_name(manifest)
            expected = f'bilibi-{container}-{module.name}'.replace('_', '-')
            if name != expected:
                raise Refused(f'{where}/{module.name}/Cargo.toml: crate {name!r} must be named {expected!r}')
            yield f'{container}/{module.name}', name


def discovered(root):
    """Map each layer to its [(relative path, crate)] list."""
    result = {}
    names = set()
    containers = {}
    for layer in LAYERS:
        if layer in CONTAINER_LAYERS:
            modules = list(container_modules(root, layer))
        else:
            modules = [(manifest.parent.name, package_name(manifest))
                       for manifest in sorted((root / 'apps' / layer).glob('*/Cargo.toml'))]
        for path, name in modules:
            if name in names:
                raise Refused(f'Duplicate module crate: {name}')
            names.add(name)
            if layer in CONTAINER_LAYERS:
                container = path.split('/')[0]
                if containers.setdefault(container, layer) != layer:
                    raise Refused(f'Container {container!r} is mounted under both apps/domains and apps/extensions')
        result[layer] = modules
    return result


def composition(layer, modules):
    text = (f'# Generated by scripts/discover-modules.py; edit deep-module manifests.\n'
            f'[package]\nname = "bilibi-{layer}"\nversion.workspace = true\n'
            'edition.workspace = true\nrust-version.workspace = true\nlicense.workspace = true\n\n[dependencies]\n')
    text += ''.join(f'{name} = {{ path = "{path}" }}\n' for path, name in modules)
    code = '// Generated composition links. Runtime inventory owns discovery.\n'
    code += ''.join(f'pub use {name.replace("-", "_")} as {path.replace("/", "_")};\n' for path, name in modules)
    code += 'pub fn link() {}\n'
    return {f'apps/{layer}/Cargo.toml': text, f'apps/{layer}/src/lib.rs': code}


def outputs(root, modules):
    result = {}
    for layer in LAYERS:
        result.update({root / path: text for path, text in composition(layer, modules[layer]).items()})
    return result


def platform_crates(root, modules):
    names = {f'bilibi-{layer}' for layer in LAYERS}
    names.update(name for layer in ('base', 'core') for _, name in modules[layer])
    for pattern in PLATFORM_MANIFESTS:
        names.update(package_name(manifest) for manifest in root.glob(pattern))
    return names


def git_versions(root, path):
    """Yield (label, text) for the committed and staged forms of a tracked file."""
    for label, spec in (('committed', f'HEAD:{path}'), ('staged', f':{path}')):
        shown = subprocess.run(['git', '-C', str(root), 'show', spec], capture_output=True, text=True)
        if shown.returncode == 0:
            yield label, shown.stdout


def check_unmounted_commit(root, modules):
    """Refuse a staged or committed composition that names mounted Domain or Extension crates."""
    inside = subprocess.run(['git', '-C', str(root), 'rev-parse', '--show-toplevel'],
                            capture_output=True, text=True)
    if inside.returncode != 0 or Path(inside.stdout.strip()).resolve() != root.resolve():
        raise Refused('--check needs the Platform Git checkout to read its staged and committed composition')
    advice = ('Never commit composition files or Cargo.lock from a mounted checkout; '
              'unmount, run python3 scripts/discover-modules.py, and let Cargo prune Cargo.lock (see apps/domains/AGENTS.md)')
    for layer in CONTAINER_LAYERS:
        for path, text in composition(layer, []).items():
            for label, shown in git_versions(root, path):
                if shown != text:
                    raise Refused(f'The {label} {path} is a mounted composition. {advice}')
    allowed = platform_crates(root, modules)
    for label, shown in git_versions(root, 'Cargo.lock'):
        lock = tomllib.loads(shown)
        foreign = sorted(p['name'] for p in lock.get('package', []) if 'source' not in p and p['name'] not in allowed)
        if foreign:
            raise Refused(f'The {label} Cargo.lock names path packages outside the Platform: {", ".join(foreign)}. {advice}')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--check', action='store_true',
                        help='refuse stale composition files and a staged or committed mounted composition')
    args = parser.parse_args()
    try:
        modules = discovered(ROOT)
        files = outputs(ROOT, modules)
        if args.check:
            for path, text in files.items():
                if not path.exists() or path.read_text() != text:
                    raise Refused(f'Composition is stale: {path}. Run python3 scripts/discover-modules.py')
            check_unmounted_commit(ROOT, modules)
        else:
            for path, text in files.items():
                path.parent.mkdir(parents=True, exist_ok=True)
                if not path.exists() or path.read_text() != text:
                    path.write_text(text)
    except Refused as refusal:
        raise SystemExit(f'discover-modules: {refusal}') from None


if __name__ == '__main__':
    main()
