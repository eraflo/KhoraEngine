// Mounts the workspace's rustdoc under the built site's `/api`.
//
// Run after `npm run build`, once rustdoc is built from the repository root:
//
//   cargo doc --workspace --no-deps
//   npm run api            (in docs/site)
//
// It copies `target/doc` into `dist/api/` and writes `dist/api/index.html`,
// a list of the crates found there (rustdoc writes no index of its own for a
// workspace). It fails, saying why, when the site or rustdoc is not built.

import { cpSync, existsSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { BASE } from '../src/book/site.mjs';

const site = fileURLToPath(new URL('../', import.meta.url));
const dist = join(site, 'dist');
const rustdoc = join(site, '..', '..', 'target', 'doc');
const api = join(dist, 'api');

/** Crates rustdoc documents that are build plumbing, not an API: left off the index. */
const PLUMBING = new Set(['workspace_hack']);

if (!existsSync(join(dist, 'index.html'))) {
    fail('the site is not built: run `npm run build` first');
}
if (!existsSync(rustdoc)) {
    fail('rustdoc is not built: run `cargo doc --workspace --no-deps` at the repository root first');
}

rmSync(api, { recursive: true, force: true });
cpSync(rustdoc, api, { recursive: true });

const crates = readdirSync(api, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && existsSync(join(api, entry.name, 'all.html')))
    .filter((entry) => !PLUMBING.has(entry.name))
    .map((entry) => entry.name)
    .sort((a, b) => (a === 'khora_sdk' ? -1 : b === 'khora_sdk' ? 1 : a.localeCompare(b)));
if (crates.length === 0) fail(`no crate found in ${rustdoc}`);

writeFileSync(join(api, 'index.html'), indexPage(crates));
console.log(`mounted rustdoc at ${BASE}/api/: ${crates.length} crates`);

/** The `/api` index: every crate rustdoc documented, the SDK first. */
function indexPage(names) {
    const items = names
        .map((name) => `<li><a href="${name}/index.html"><code>${name}</code></a></li>`)
        .join('\n');
    return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>API reference | Khora</title>
<link rel="icon" href="${BASE}/logos/khora-icon.svg" type="image/svg+xml">
<style>
  body { margin: 0; background: oklch(0.16 0.022 265); color: oklch(0.82 0.01 250);
         font: 15px/1.65 system-ui, sans-serif; }
  main { max-width: 46rem; margin: 0 auto; padding: 4rem 1rem; }
  h1 { color: oklch(0.96 0.005 250); font-weight: 400; font-size: 2.4rem; margin: 0 0 .5rem; }
  ul { list-style: none; padding: 0; margin: 2rem 0; border-top: 1px solid oklch(1 0 0 / .06); }
  li { border-bottom: 1px solid oklch(1 0 0 / .06); }
  a { display: block; padding: .7rem .25rem; color: oklch(0.84 0.04 240); text-decoration: none; }
  a:hover { background: oklch(0.84 0.04 240 / .08); }
  code { font: 14px ui-monospace, Consolas, monospace; }
</style>
</head>
<body>
<main>
<h1>API reference</h1>
<p>The rustdoc of every crate in the workspace. A game uses <code>khora_sdk</code>; the others are the engine's own. <a href="${BASE}/home/" style="display:inline;padding:0">Back to the docs</a>.</p>
<ul>
${items}
</ul>
</main>
</body>
</html>
`;
}

function fail(message) {
    console.error(`mount-api: ${message}`);
    process.exit(1);
}
