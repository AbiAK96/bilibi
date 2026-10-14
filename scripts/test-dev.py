#!/usr/bin/env python3
"""Executable launcher/Caddy behavior tests. Never sudo or touch real /etc/caddy."""
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
        self.caddy = self.fixture / 'etc/caddy'
        self.sites = self.caddy / 'bilibi'
        self.certs = self.sites / 'certs'
        self.certs.mkdir(parents=True)
        self.main = self.caddy / 'Caddyfile'
        self.main.write_text('# Existing shared Caddy\n')
        self.hosts = self.fixture / 'etc/hosts'
        self.hosts.write_text('127.0.0.1 localhost\n')
        self.checkout = self.fixture / 'checkout'
        https = self.checkout / '.local/https'
        https.mkdir(parents=True)
        (https / 'bilibi.localhost.pem').write_text('new leaf')
        (https / 'bilibi.localhost-key.pem').write_text('new key')
        self.bin = self.fixture / 'bin'
        self.bin.mkdir()
        executable(self.bin / 'ss', '[[ ${BILIBI_OCCUPIED:-} != "${*: -1}" ]] || echo "LISTEN occupied"\n')
        executable(self.bin / 'caddy', '[[ ${BILIBI_FAIL_VALIDATE:-0} != 1 ]]\n')
        executable(self.bin / 'getent', 'exit 1\n')
        executable(self.bin / 'systemctl', '[[ ${BILIBI_FAIL_RELOAD:-0} != 1 ]]\n')
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}',
                        BILIBI_HTTPS_TEST='1', BILIBI_CADDY_ROOT=str(self.caddy),
                        BILIBI_HOSTS_FILE=str(self.hosts), BILIBI_CADDY_LOCK=str(self.fixture / 'lock'))

    def tearDown(self):
        self.temporary.cleanup()

    def install(self, port='4300', pinned='0', **env):
        return subprocess.run(['bash', str(ROOT / 'scripts/local-https/install-caddy.sh'),
                               str(self.checkout), 'bilibi.localhost', port, '127.0.0.1 ::1', pinned],
                              env=dict(self.env, **env), capture_output=True, text=True)

    def test_ports_coexist_with_bilimbi_and_repeat_is_stable(self):
        other = self.caddy / 'bilimbi'
        other.mkdir()
        fragment = other / 'bilimbi.localhost.caddy'
        fragment.write_text(f'# Bilimbi checkout: {ROOT}\nbilimbi.localhost {{ reverse_proxy 127.0.0.1:4300 }}\n')
        previous = fragment.read_bytes()
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), '4301')
        self.assertEqual(fragment.read_bytes(), previous)
        result = self.install('4301')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), '4301')
        self.assertEqual(self.main.read_text().count('import '), 1)
        self.assertEqual(self.install('4300', '1').returncode, 1)
        self.assertEqual((self.sites / 'bilibi.localhost.caddy').read_text().splitlines()[0], f'# Bilibi checkout: {self.checkout}')

    def test_validation_and_reload_failure_roll_back_every_owned_file(self):
        self.assertEqual(self.install().returncode, 0)
        installed = [self.main, self.sites / 'bilibi.localhost.caddy',
                     self.certs / 'bilibi.localhost.pem', self.certs / 'bilibi.localhost-key.pem']
        snapshot = [path.read_bytes() for path in installed]
        (self.checkout / '.local/https/bilibi.localhost.pem').write_text('replacement')
        for failure in ('BILIBI_FAIL_VALIDATE', 'BILIBI_FAIL_RELOAD'):
            result = self.install('4302', **{failure: '1'})
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual([path.read_bytes() for path in installed], snapshot)

    def test_stale_sites_are_pruned_only_after_success_and_host_can_be_reused(self):
        stale = self.sites / 'bilibi.localhost.caddy'
        stale.write_text(f'# Bilibi checkout: {self.fixture / "gone"}\nbilibi.localhost {{ reverse_proxy 127.0.0.1:4300 }}\n')
        (self.certs / 'bilibi.localhost.pem').write_text('stale leaf')
        (self.certs / 'bilibi.localhost-key.pem').write_text('stale key')
        self.hosts.write_text('127.0.0.1\tbilibi.localhost # Bilibi local HTTPS\n')
        original = stale.read_bytes()
        result = self.install(BILIBI_FAIL_VALIDATE='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(stale.read_bytes(), original)
        self.assertEqual((self.certs / 'bilibi.localhost.pem').read_text(), 'stale leaf')
        self.assertIn('bilibi.localhost', self.hosts.read_text())
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(stale.read_text().splitlines()[0], f'# Bilibi checkout: {self.checkout}')
        self.assertEqual((self.certs / 'bilibi.localhost.pem').read_text(), 'new leaf')

    def test_owner_conflict_and_main_file_port_reservation(self):
        fragment = self.sites / 'bilibi.localhost.caddy'
        fragment.write_text(f'# Bilibi checkout: {ROOT}\nbilibi.localhost {{ reverse_proxy 127.0.0.1:4300 }}\n')
        original = fragment.read_bytes()
        self.assertEqual(self.install().returncode, 1)
        self.assertEqual(fragment.read_bytes(), original)
        fragment.unlink()
        self.main.write_text('belimbing.localhost { reverse_proxy 127.0.0.1:4300 }\n')
        result = self.install()
        self.assertEqual(result.stdout.strip(), '4301', result.stderr)

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
        scripts.mkdir()
        shutil.copy(ROOT / 'scripts/start-app.sh', scripts / 'start-app.sh')
        shutil.copy(ROOT / 'scripts/discover-modules.py', scripts / 'discover-modules.py')
        executable(scripts / 'local-https.sh', 'mkdir -p .local/https; printf "bilibi.localhost\\n4305\\n" > .local/https/instance\n')
        for name in ('rustc', 'pg_isready', 'psql', 'mkcert', 'sudo', 'curl', 'openssl'):
            executable(self.bin / name, 'echo fixture\n')
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
                                env=dict(self.env, DATABASE_URL='postgres://fixture/db', SEED_PASSWORD='fixture-password'),
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('database migrate', result.stdout)
        self.assertIn('database seed', result.stdout)
        self.assertIn('Rust build took', result.stdout)
        self.assertIn('watched https://bilibi.localhost 127.0.0.1:4305', result.stdout)

    def test_real_caddy_consumes_generated_configuration(self):
        real = shutil.which('caddy', path=os.environ['PATH'])
        if not real:
            self.skipTest('Install Caddy to validate the generated site with the real consumer')
        https = self.checkout / '.local/https'
        subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
                        '-subj', '/CN=bilibi.localhost', '-keyout', str(https / 'bilibi.localhost-key.pem'),
                        '-out', str(https / 'bilibi.localhost.pem')], check=True, capture_output=True)
        self.assertEqual(self.install().returncode, 0)
        result = subprocess.run([real, 'validate', '--config', str(self.main), '--adapter', 'caddyfile'],
                                env=dict(os.environ, XDG_DATA_HOME=str(self.fixture / 'data'),
                                         XDG_CONFIG_HOME=str(self.fixture / 'config')), capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

if __name__ == '__main__':
    unittest.main()
