// Builds the solver for the browser and copies it to src/wasm/hexas.wasm (git-ignored).
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
execFileSync('cargo', ['build', '-p', 'hexas-wasm', '--release', '--target', 'wasm32-unknown-unknown'], {
  cwd: root,
  stdio: 'inherit',
});
const out = resolve(root, 'web/src/wasm');
mkdirSync(out, { recursive: true });
copyFileSync(resolve(root, 'target/wasm32-unknown-unknown/release/hexas_wasm.wasm'), resolve(out, 'hexas.wasm'));
console.log('copied hexas.wasm to web/src/wasm/');
