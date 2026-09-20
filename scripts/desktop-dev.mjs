import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';

function candidateCargoBins() {
  const homes = [process.env.CARGO_HOME];

  if (process.platform === 'win32') {
    homes.push(process.env.USERPROFILE && path.join(process.env.USERPROFILE, '.cargo'));
  }

  homes.push(process.env.HOME && path.join(process.env.HOME, '.cargo'));

  return homes
    .filter((value) => typeof value === 'string' && value.length > 0)
    .map((home) => path.join(home, 'bin'));
}

const env = { ...process.env };
const cargoBin = candidateCargoBins().find((candidate) => existsSync(candidate));

if (cargoBin && !(env.PATH ?? '').split(path.delimiter).includes(cargoBin)) {
  env.PATH = `${cargoBin}${path.delimiter}${env.PATH ?? ''}`;
}

const tauriBin =
  process.platform === 'win32'
    ? path.join(process.cwd(), 'node_modules', '.bin', 'tauri.cmd')
    : path.join(process.cwd(), 'node_modules', '.bin', 'tauri');

if (!existsSync(tauriBin)) {
  console.error('Missing local Tauri CLI. Run "npm install" first.');
  process.exit(1);
}

const child =
  process.platform === 'win32'
    ? spawn(`"${tauriBin}" dev`, {
        stdio: 'inherit',
        env,
        shell: true,
      })
    : spawn(tauriBin, ['dev'], {
        stdio: 'inherit',
        env,
      });

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
