// One-shot deterministic build. The application embeds the committed output;
// Node and npm are build tools only. --check compares bytes without writing it.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { execFileSync } = require('node:child_process');

// Mounted Domain and Extension containers are their own Git checkouts, ignored by
// the Platform's .gitignore, and Tailwind skips ignored paths even under a listed
// directory. Scan each mounted container explicitly, so a class used only by a
// Domain either exists in the committed CSS or fails --check.
const apps = path.join(__dirname, '../../..');
const mounted = ['domains', 'extensions'].flatMap(layer => {
  const root = path.join(apps, layer);
  return fs.readdirSync(root, { withFileTypes: true })
    .filter(entry => entry.isDirectory() && !entry.name.startsWith('.')
      && fs.existsSync(path.join(root, entry.name, 'bilibi.container.toml')))
    .map(entry => path.join(root, entry.name)).sort();
});
const entry = path.join(__dirname, 'css', `.mounted-${process.pid}.css`);
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'bilibi-css-'));
try {
  const output = path.join(directory, 'app.css');
  fs.writeFileSync(entry, ['@import "./app.css";', ...mounted.map(dir => `@source ${JSON.stringify(dir)};`)].join('\n') + '\n');
  execFileSync(process.execPath, [
    path.join(__dirname, 'node_modules/@tailwindcss/cli/dist/index.mjs'),
    '--input', entry,
    '--output', output,
    '--minify',
  ], { cwd: __dirname, stdio: 'inherit' });
  const compiled = fs.readFileSync(output);
  const artifact = path.join(__dirname, 'app.css');
  if (process.argv.includes('--check')) {
    if (!fs.existsSync(artifact) || !compiled.equals(fs.readFileSync(artifact))) {
      console.error('UI CSS is stale. Run npm run build --prefix apps/base/ui/assets and commit app.css. A class used only by a mounted Domain belongs in a Platform change made with nothing mounted: Domains own no CSS.');
      process.exitCode = 1;
    }
  } else {
    // Leave an identical file untouched: the dev watcher sees every write,
    // even a byte-identical one, and would rebuild again.
    if (!fs.existsSync(artifact) || !compiled.equals(fs.readFileSync(artifact))) {
      fs.writeFileSync(artifact, compiled);
    }
  }
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
  fs.rmSync(entry, { force: true });
}
