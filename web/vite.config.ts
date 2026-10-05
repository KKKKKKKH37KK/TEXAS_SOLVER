/// <reference types="vitest/config" />
import { createReadStream, existsSync, statSync } from 'node:fs';
import { resolve, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig, type Connect, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';

/**
 * Serves the flop library (repo-root `library/`, built by `hexas library`, git-ignored) at
 * `./library/` in `vite dev` and `vite preview`. The published site has no library yet.
 */
function localLibrary(): Plugin {
  const root = fileURLToPath(new URL('../library', import.meta.url));
  const serve: Connect.NextHandleFunction = (req, res, next) => {
    const url = decodeURIComponent((req.url ?? '').split('?')[0]);
    const i = url.indexOf('/library/');
    if (i < 0) return next();
    const file = normalize(resolve(root, '.' + url.slice(i + '/library'.length)));
    if (!file.startsWith(root) || !existsSync(file) || !statSync(file).isFile()) return next();
    res.setHeader('Content-Type', 'application/octet-stream');
    createReadStream(file).pipe(res);
  };
  return {
    name: 'local-library',
    configureServer: (s) => void s.middlewares.use(serve),
    configurePreviewServer: (s) => void s.middlewares.use(serve),
  };
}

export default defineConfig({
  // Relative base so the build also works from GitHub Pages or a local file server.
  base: './',
  plugins: [react(), localLibrary()],
  worker: { format: 'es' },
  test: {
    environment: 'node',
    include: ['tests/**/*.test.ts'],
  },
});
