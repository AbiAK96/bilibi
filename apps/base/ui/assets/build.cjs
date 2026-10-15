// One-shot deterministic build. The application embeds the committed output;
// Node and npm are build tools only. --check compares bytes without writing it.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { execFileSync } = require('node:child_process');

const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'bilibi-css-'));
try {
  const output = path.join(directory, 'app.css');
  execFileSync(process.execPath, [
    path.join(__dirname, 'node_modules/@tailwindcss/cli/dist/index.mjs'),
    '--input', path.join(__dirname, 'css/app.css'),
    '--output', output,
    '--minify',
  ], { cwd: __dirname, stdio: 'inherit' });
  const compiled = fs.readFileSync(output);
  const artifact = path.join(__dirname, 'app.css');
  if (process.argv.includes('--check')) {
    if (!fs.existsSync(artifact) || !compiled.equals(fs.readFileSync(artifact))) {
      console.error('UI CSS is stale. Run npm run build --prefix apps/base/ui/assets and commit app.css.');
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
}
