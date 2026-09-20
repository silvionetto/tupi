import fs from 'node:fs';
import path from 'node:path';

const version = process.argv[2]?.trim();

if (!version) {
  console.error('Usage: node scripts/set-version.mjs <version>');
  process.exit(1);
}

const root = process.cwd();

function updateJson(relativePath, mutator) {
  const filePath = path.join(root, relativePath);
  const parsed = JSON.parse(fs.readFileSync(filePath, 'utf8'));
  mutator(parsed);
  fs.writeFileSync(filePath, `${JSON.stringify(parsed, null, 2)}\n`);
}

function replaceVersion(relativePath, pattern) {
  const filePath = path.join(root, relativePath);
  const contents = fs.readFileSync(filePath, 'utf8');
  const updated = contents.replace(pattern, `$1${version}$3`);

  if (contents === updated) {
    throw new Error(`Could not update version in ${relativePath}`);
  }

  fs.writeFileSync(filePath, updated);
}

updateJson('package.json', (pkg) => {
  pkg.version = version;
});

updateJson('src-tauri/tauri.conf.json', (config) => {
  config.package.version = version;
});

replaceVersion('src-tauri/Cargo.toml', /^(version\s*=\s*")([^"]+)(")$/m);
