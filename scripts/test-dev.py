#!/usr/bin/env python3
"""Offline checks for module discovery and local launcher orchestration."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent


def executable(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('#!/usr/bin/env bash\nset -eu\n' + text)
    path.chmod(0o755)


class DevTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='bilibi-dev-', dir='/tmp')
        self.fixture = Path(self.temporary.name)
        self.checkout = self.fixture / 'checkout'
        self.bin = self.fixture / 'bin'
        self.bin.mkdir()
        for name in ('rustc', 'psql', 'pg_isready', 'caddy', 'mkcert', 'openssl', 'curl', 'flock'):
            executable(self.bin / name, 'echo fixture\n')
        executable(self.bin / 'ss', ':\n')
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}')

    def tearDown(self):
        self.temporary.cleanup()

    def test_generated_composition_admits_a_new_deep_module(self):
        spec = importlib.util.spec_from_file_location('discovery', ROOT / 'scripts/discover-modules.py')
        discovery = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(discovery)
        manifest = self.fixture / 'apps/domains/example/Cargo.toml'
        manifest.parent.mkdir(parents=True)
        manifest.write_text('[package]\nname="example-domain"\n')
        outputs = discovery.outputs(self.fixture)
        value = tomllib.loads(outputs[self.fixture / 'apps/domains/Cargo.toml'])
        self.assertEqual(value['dependencies']['example-domain']['path'], 'example')
        for layer in discovery.LAYERS:
            value = tomllib.loads(outputs[self.fixture / 'apps' / layer / 'Cargo.toml'])
            self.assertEqual(value['package']['name'], f'bilibi-{layer}')

    def test_launcher_prepares_database_and_preserves_https_environment(self):
        scripts = self.checkout / 'scripts'
        scripts.mkdir(parents=True)
        shutil.copy(ROOT / 'scripts/start-app.sh', scripts / 'start-app.sh')
        shutil.copy(ROOT / 'scripts/discover-modules.py', scripts / 'discover-modules.py')
        executable(scripts / 'local-https.sh', 'mkdir -p .local/https; printf "bilibi.localhost\\n4305\\n" > .local/https/instance\n')
        executable(self.bin / 'cargo', '''case "$*" in
  *--version*) echo cargo-fixture;;
  build*) mkdir -p target/debug
          printf '#!/bin/sh\\nprintf "database %%s\\\\n" "$1"\\n' > target/debug/bilibi-web
          chmod +x target/debug/bilibi-web;;
  watch*) printf 'watched %s %s\\n' "$APP_ORIGIN" "$BIND_ADDR";;
  *) exit 1;;
esac
''')
        result = subprocess.run(['bash', str(scripts / 'start-app.sh')], cwd=self.checkout,
                                env=dict(self.env, DATABASE_URL='postgres://fixture/db'),
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('database migrate', result.stdout)
        self.assertIn('database seed', result.stdout)
        self.assertIn('Rust build took', result.stdout)
        self.assertIn('watched https://bilibi.localhost 127.0.0.1:4305', result.stdout)


if __name__ == '__main__':
    unittest.main()
