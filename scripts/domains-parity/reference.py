#!/usr/bin/env python3
"""Export the pinned Bilimbi reference and capture its domain pages (no unit tests)."""
import argparse
import difflib
import hashlib
import io
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import tarfile
from datetime import datetime, timezone

PINS = {
    "bilimbi": "d48e3423a6df733cfe546b887c8512af4eb7fc82",
    "b-dom-supply-chain": "4b4f95c8effbd719077b35b9f792f3de413d8dba",
    "b-dom-finance": "4abfaf829321f8540581d833752a086b21b45ba9",
    "b-dom-factory": "f39ed3dcf2ce2694d30c2b88e82212780f4f3b98",
}
MISSING_ID = "2147483647"
HERE = Path(__file__).resolve().parent


def command(args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, text=True, **kwargs)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def export(args):
    evidence = args.evidence
    reference = evidence / "reference"
    if reference.exists():
        raise SystemExit("Reference exists; use a fresh evidence directory to preserve source pins")
    for name, sha in PINS.items():
        source = args.projects / name
        actual = subprocess.check_output(["git", "-C", str(source), "rev-parse", sha], text=True).strip()
        if actual != sha:
            raise SystemExit(f"Unexpected source revision: {name}")
        destination = reference
        if name != "bilimbi":
            destination /= "apps/domains/" + name.removeprefix("b-dom-").replace("-", "_")
        destination.mkdir(parents=True, exist_ok=True)
        archive = subprocess.check_output(["git", "-C", str(source), "archive", sha])
        with tarfile.open(fileobj=io.BytesIO(archive)) as files:
            files.extractall(destination, filter="data")
    for filename in ("fixtures.exs", "serve.exs", "walkthrough.exs"):
        (evidence / filename).write_bytes((HERE / filename).read_bytes())
    identity = {"email": "parity-operator@example.invalid", "password": secrets.token_urlsafe(32)}
    identity_path = evidence / "identity.json"
    with open(identity_path, "x", opener=lambda path, flags: os.open(path, flags, 0o600)) as file:
        json.dump(identity, file)
    write_json(evidence / "pins.json", PINS)
    tls(args)
    routes(args)


def routes(args):
    """Read only literal route contribution records from the pinned source."""
    found = []
    root = args.evidence / "reference/apps/domains"
    for source in sorted(root.glob("*/*/priv/web_routes.exs")):
        module = "/".join(source.relative_to(root).parts[:2])
        for block in re.findall(r"%\{(.*?)\}", source.read_text(), re.S):
            path = re.search(r'path: "([^"]+)"', block)[1]
            kind = "live" if re.search(r"\blive:", block) else "controller"
            concrete = re.sub(r":\w+", MISSING_ID, path)
            found.append({"module": module, "path": path, "capture_path": concrete,
                          "kind": kind, "state": "missing-record" if ":" in path else "empty",
                          "capability": re.search(r'capability: "([^"]+)"', block)[1]})
    if len(found) != 49 or sum(route["kind"] == "live" for route in found) != 44:
        raise SystemExit("Pinned inventory must have 49 routes, including 44 LiveViews")
    write_json(args.evidence / "routes.json", found)
    print("Inventoried 44 LiveViews and 5 controllers")


def repair(args):
    """Apply the approved, recorded repair only in the exported reference."""
    relative = "apps/domains/finance/reconciliation/lib/reconciliation/contributions.ex"
    source = args.evidence / "reference" / relative
    before = source.read_text()
    old = 'verbs: ["import", "finalise"]'
    new = 'verbs: ["finalise"]'
    if before.count(old) != 1:
        raise SystemExit("Expected pinned declaration missing; preserve the reference and inspect it")
    after = before.replace(old, new)
    patch = "".join(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
                                       fromfile="a/" + relative, tofile="b/" + relative))
    (args.evidence / "reference-compatibility.patch").write_text(patch)
    source.write_text(after)
    write_json(args.evidence / "reference-repair.json", {
        "approved_by": "firstmate, steering message 001, 2026-10-16T04:17:53Z",
        "defect": "finance/reconciliation and factory/production_execution both register import",
        "change": "Remove redundant reconciliation verb registration; all capability keys remain",
        "source": relative, "before_sha256": hashlib.sha256(before.encode()).hexdigest(),
        "after_sha256": hashlib.sha256(after.encode()).hexdigest(),
    })
    print("Applied approved reference-only import registration repair")


def tls(args):
    destination = args.evidence / "tls"
    destination.mkdir(mode=0o700, exist_ok=True)
    if (destination / "key.pem").exists():
        raise SystemExit("TLS identity exists; keep the reference identity instead of replacing it")
    command(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "30",
             "-subj", "/CN=127.0.0.1", "-addext", "subjectAltName=IP:127.0.0.1",
             "-keyout", destination / "key.pem", "-out", destination / "cert.pem"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    (destination / "key.pem").chmod(0o600)


def dump(args):
    """Keep pg_dump output plus the trigger routines pg_dump -t omits."""
    destination = args.evidence / "schema"
    destination.mkdir(exist_ok=True)
    database = os.environ["DATABASE_URL"]
    pg = Path(args.pg_bin)
    query = lambda sql: subprocess.check_output(
        [str(pg / "psql"), database, "-XAt", "-v", "ON_ERROR_STOP=1", "-c", sql], text=True)
    with (destination / "all.sql").open("w") as file:
        command([pg / "pg_dump", database, "--schema-only", "--no-owner", "--no-privileges"], stdout=file)
    inventory = []
    for descriptor in sorted((args.evidence / "reference/apps/domains").glob("*/*/bilimbi.module.exs")):
        container, module = descriptor.parts[-3:-1]
        prefix = f"{container}_{module}_"
        pattern = re.escape(prefix).replace("_", "[_]")
        # Factory predates module-qualified table names. Read each owner's
        # literal Ecto schema names instead of assigning every factory_* table
        # to Inventory or silently omitting Production Execution's tables.
        owned = set()
        for source in descriptor.parent.rglob("*.ex"):
            owned.update(re.findall(r'\bschema\s+"([a-z_]+)"', source.read_text()))
        names = ",".join("'" + table + "'" for table in sorted(owned))
        if not names:
            raise SystemExit(f"No declared schemas for {container}/{module}")
        tables = query("SELECT tablename FROM pg_tables WHERE schemaname='public' "
                       f"AND tablename IN ({names}) ORDER BY tablename").splitlines()
        if set(tables) != owned:
            raise SystemExit(f"Missing migrated tables for {container}/{module}: {owned - set(tables)}")
        if not tables:
            raise SystemExit(f"No migrated tables for {container}/{module}")
        output = destination / f"{container}-{module}.sql"
        with output.open("w") as file:
            command([pg / "pg_dump", database, "--schema-only", "--no-owner", "--no-privileges",
                     *[arg for table in tables for arg in ("-t", f"public.{table}")]], stdout=file)
        # Include all named module functions, plus any function actually attached to its triggers.
        names = ",".join("'public." + table + "'::regclass" for table in tables)
        declared_functions = set()
        for migration in (descriptor.parent / "priv/repo/migrations").glob("*.exs"):
            declared_functions.update(re.findall(
                r"CREATE\s+(?:OR\s+REPLACE\s+)?FUNCTION\s+([a-z_]+)",
                migration.read_text(), re.I))
        function_names = ",".join("'" + name + "'" for name in sorted(declared_functions)) or "NULL"
        function_sql = ("SELECT pg_get_functiondef(p.oid) FROM pg_proc p "
                        "JOIN pg_namespace n ON p.pronamespace=n.oid "
                        "WHERE n.nspname='public' AND ("
                        f"p.proname ~ '^{pattern}' OR p.proname IN ({function_names}) OR p.oid IN "
                        f"(SELECT tgfoid FROM pg_trigger WHERE tgrelid IN ({names}) AND NOT tgisinternal)) "
                        "ORDER BY p.proname, p.oid")
        functions = destination / f"{container}-{module}.functions.sql"
        functions.write_text("SET check_function_bodies = false;\n" + query(function_sql))
        inventory.append({"module": f"{container}/{module}", "tables": tables,
                          "pg_dump": output.name, "functions": functions.name,
                          "sha256": hashlib.sha256(output.read_bytes()).hexdigest()})
    write_json(destination / "inventory.json", inventory)
    print(f"Dumped {len(inventory)} modules, {sum(len(row['tables']) for row in inventory)} tables")


def capture(args):
    """All browser operations go through an isolated chrome-devtools-axi session."""
    evidence = args.evidence
    screens = evidence / "screens" / args.theme
    screens.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CHROME_DEVTOOLS_AXI_SESSION=args.session, TZ="Asia/Kuala_Lumpur")
    if args.base_url.startswith("https://127.0.0.1:"):
        # A private self-signed endpoint keeps Bilimbi's compiled SSL policy.
        # This flag applies only to this task's isolated browser process.
        env["CHROME_DEVTOOLS_AXI_CHROME_ARGS"] = (
            env.get("CHROME_DEVTOOLS_AXI_CHROME_ARGS", "") + " --ignore-certificate-errors").strip()
    def axi(*argv, script=None):
        try:
            return command(["chrome-devtools-axi", *argv], env=env, input=script, capture_output=True).stdout
        except subprocess.CalledProcessError as error:
            raise SystemExit(error.stderr or error.stdout or str(error)) from error
    # Explicit selection also works when the CLI cannot infer a unique page
    # after redirects or when several tabs carry the same URL.
    pages = axi("pages")
    page_ids = re.findall(r"^\s*(\d+),", pages, re.M)
    if not page_ids:
        raise SystemExit("The isolated browser has no selectable tab")
    axi("selectpage", page_ids[0])
    axi("emulate", "--viewport", "1440x1000x1", "--color-scheme", args.theme)
    identity = json.loads((evidence / "identity.json").read_text())
    # Login uses the real HTTP form; no session injection or authentication bypass.
    login = f"""
await page.open({json.dumps(args.base_url + '/')});
await page.eval(async () => {{
  const deadline = Date.now() + 15000;
  while (!document.querySelector('.phx-connected') && Date.now() < deadline)
    await new Promise(resolve => setTimeout(resolve, 100));
  if (!document.querySelector('.phx-connected')) throw new Error('Login LiveView did not connect');
}});
console.log(await page.snapshot());
"""
    (evidence / "login-snapshot.txt").write_text(axi("run", script=login))
    login = f"""
if (await page.eval(() => !!document.querySelector('#login-email'))) {{
  // Selector-based fill/click helpers in this installed CLI pass an IIFE
  // where MCP expects a function. Use its working eval API for DOM events.
  await page.eval(() => {{
    for (const [selector, value] of [
      ['#login-email', {json.dumps(identity['email'])}],
      ['#login-password', {json.dumps(identity['password'])}]
    ]) {{
      const input = document.querySelector(selector);
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
      input.dispatchEvent(new Event('input', {{bubbles: true}}));
      input.dispatchEvent(new Event('change', {{bubbles: true}}));
    }}
    document.querySelector('#login-submit').click();
  }});
  await page.eval(async () => {{
    const deadline = Date.now() + 15000;
    while (location.pathname === '/' && Date.now() < deadline)
      await new Promise(resolve => setTimeout(resolve, 100));
    if (location.pathname === '/') throw new Error('Login did not finish');
  }});
}}
console.log(await page.snapshot());
"""
    (evidence / "authenticated-snapshot.txt").write_text(axi("run", script=login))
    rows = []
    for route in json.loads((evidence / "routes.json").read_text()):
        if route["kind"] != "live":
            continue
        path = route["capture_path"]
        filename = route["path"].strip("/").replace("/", "__").replace(":", "")
        script = f"""
const response = await page.open({json.dumps(args.base_url + path)});
await page.eval(async () => {{
  await document.fonts.ready;
  const deadline = Date.now() + 15000;
  while (!document.querySelector('.phx-connected') && Date.now() < deadline)
    await new Promise(resolve => setTimeout(resolve, 100));
  if (!document.querySelector('.phx-connected')) throw new Error('LiveView did not connect');
}});
console.log(JSON.stringify({{response, location: await page.eval(() => location.pathname),
  title: await page.eval(() => document.title),
  timezone: await page.eval(() => Intl.DateTimeFormat().resolvedOptions().timeZone)}}));
console.log(await page.snapshot());
"""
        snapshot = axi("run", script=script)
        (screens / f"{filename}.txt").write_text(snapshot)
        # Do not certify an auth redirect or an error page as a successful capture.
        if f'"location":"{path}"' not in snapshot:
            raise SystemExit(f"Route redirected unexpectedly: {path}")
        axi("screenshot", str(screens / f"{filename}.png"), "--full-page")
        rows.append({**route, "screenshot": f"screens/{args.theme}/{filename}.png"})
        write_json(screens / "manifest.json", {"captured_at": datetime.now(timezone.utc).isoformat(),
                   "theme": args.theme, "viewport": "1440x1000@1", "routes": rows})
        print(f"Captured {len(rows)}/44: {route['path']}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("export", "routes", "repair", "tls", "dump", "capture"))
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--projects", type=Path)
    parser.add_argument("--pg-bin", default="/usr/lib/postgresql/18/bin")
    parser.add_argument("--base-url", default="https://127.0.0.1:41461")
    parser.add_argument("--theme", choices=("light", "dark"), default="light")
    parser.add_argument("--session", default=f"bilibi-dom-r1-{int(datetime.now().timestamp())}")
    args = parser.parse_args()
    args.evidence = args.evidence.resolve()
    if args.evidence.is_relative_to(HERE.parents[1]):
        parser.error("Evidence must be outside the Bilibi repository")
    if args.action == "export" and args.projects is None:
        parser.error("export requires --projects")
    globals()[args.action](args)


if __name__ == "__main__":
    main()
