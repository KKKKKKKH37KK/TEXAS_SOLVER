// Uploads the flop library (repo-root library/<line>/) to a Hugging Face dataset repo, skipping
// files that are already there. The web app reads them from
//   https://huggingface.co/datasets/<repo>/resolve/main/<line>/<flop>.hxs
//
// Usage (PowerShell, in your own terminal so the token stays out of logs):
//   $env:HF_TOKEN = "hf_..."            # a write token from https://huggingface.co/settings/tokens
//   $env:HF_REPO  = "<user>/hexas-library"
//   npm run upload -- srp-btn-bb [3bp-bb-btn ...]
import { createRepo, listFiles, repoExists, uploadFiles } from '@huggingface/hub';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const accessToken = process.env.HF_TOKEN;
const name = process.env.HF_REPO;
const lines = process.argv.slice(2);
if (!accessToken || !name || lines.length === 0) {
  console.error('set HF_TOKEN and HF_REPO, then: npm run upload -- <line> [<line> ...]');
  process.exit(1);
}
const repo = { type: 'dataset', name };
const libraryDir = fileURLToPath(new URL('../../library', import.meta.url));

if (!(await repoExists({ repo, accessToken }))) {
  console.log(`creating public dataset ${name}`);
  await createRepo({ repo, accessToken, visibility: 'public' });
}

const present = new Map();
for await (const f of listFiles({ repo, accessToken, recursive: true })) {
  if (f.type === 'file') present.set(f.path, f.size);
}

for (const line of lines) {
  const dir = join(libraryDir, line);
  const names = readdirSync(dir).filter((f) => f.endsWith('.hxs') || f === 'line.json' || f === 'index.csv');
  // Result files never change once written; index.csv grows, so always refresh it.
  const todo = names.filter((f) => f === 'index.csv' || !present.has(`${line}/${f}`));
  console.log(`${line}: ${names.length} files, ${todo.length} to upload`);
  const batch = 200;
  for (let i = 0; i < todo.length; i += batch) {
    const files = todo.slice(i, i + batch).map((f) => ({
      path: `${line}/${f}`,
      content: new Blob([readFileSync(join(dir, f))]),
    }));
    await uploadFiles({
      repo,
      accessToken,
      files,
      commitTitle: `${line}: ${files.length} files`,
    });
    console.log(`  uploaded ${Math.min(i + batch, todo.length)}/${todo.length}`);
  }
}
console.log(`done: https://huggingface.co/datasets/${name}`);
