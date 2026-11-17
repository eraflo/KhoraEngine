// The content loader for the book: every page of `docs/src`, where it is.
//
// Astro's `glob()` loader cannot change a page before rendering it, and the
// pages are mdBook pages (see `page.mjs`). This loader lists them, prepares
// each in memory and renders it with the site's Markdown processor
// (`renderMarkdown`, the public loader API), so whichever processor Astro
// runs, the page is read the same way. Nothing is written back to
// `docs/src`.
//
// A page is rendered again only when its prepared text changes: the digest
// covers the figures it includes, not only the page file. In `astro dev`,
// any change under `docs/src` reloads the book.

import { readFileSync, readdirSync } from 'node:fs';
import { join, posix, relative, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { preparePage } from './page.mjs';
import { pageId, parseSummary, summaryPaths } from './summary.mjs';

/**
 * Creates the loader of the `docs` collection.
 *
 * @param {object} options
 * @param {string} options.srcDir The absolute path of `docs/src`.
 * @param {string} options.base The site base, e.g. `/KhoraEngine` (no trailing `/`).
 * @returns {import('astro/loaders').Loader}
 */
export function bookLoader({ srcDir, base }) {
    return {
        name: 'khora-book-loader',
        load: async (context) => {
            let running = Promise.resolve();
            const reload = () => {
                running = running.then(() => syncBook(context, srcDir, base)).catch((error) => {
                    context.logger.error(error.message);
                });
                return running;
            };
            await syncBook(context, srcDir, base);
            if (!context.watcher) return;
            context.watcher.add(srcDir);
            context.watcher.on('all', (_, changed) => {
                if (!relative(srcDir, changed).startsWith('..')) reload();
            });
        },
    };
}

/**
 * Lists the pages under `srcDir`: every `.md` file but `SUMMARY.md`.
 *
 * @param {string} srcDir
 * @returns {string[]} Paths relative to `srcDir`, `/` separated, sorted.
 */
export function listPages(srcDir) {
    return readdirSync(srcDir, { recursive: true, withFileTypes: true })
        .filter((entry) => entry.isFile() && entry.name.endsWith('.md'))
        .map((entry) => relative(srcDir, join(entry.parentPath, entry.name)).split(sep).join('/'))
        .filter((path) => path !== 'SUMMARY.md')
        .sort();
}

async function syncBook(context, srcDir, base) {
    const { store, parseData, renderMarkdown, generateDigest, config } = context;
    const summary = parseSummary(readFileSync(join(srcDir, 'SUMMARY.md'), 'utf8'));
    const labels = new Map();
    const collect = (entry) => {
        labels.set(entry.path, entry.label);
        entry.children.forEach(collect);
    };
    summary.forEach((node) => (node.kind === 'part' ? node.items : [node]).forEach(collect));
    const listed = new Set(summaryPaths(summary));
    const root = fileURLToPath(config.root);
    const untouched = new Set(store.keys());

    for (const path of listPages(srcDir)) {
        if (!listed.has(path)) {
            context.logger.warn(`${path} is not in SUMMARY.md: the site, like mdBook, leaves it out`);
            continue;
        }
        const file = join(srcDir, path);
        const { title, body } = preparePage({
            text: readFileSync(file, 'utf8'),
            path,
            srcDir,
            base,
            label: labels.get(path),
        });
        const id = pageId(path);
        untouched.delete(id);
        const digest = generateDigest(`${title}\n${body}`);
        if (store.get(id)?.digest === digest) continue;
        const filePath = posix.normalize(relative(root, file).split(sep).join('/'));
        const data = await parseData({ id, data: { title }, filePath });
        // The `slug` tells the Markdown plugins which page this is (the links
        // validator keys its pages by it); the page's data comes from parseData.
        const rendered = await renderMarkdown(`---\nslug: ${JSON.stringify(id)}\n---\n${body}`, {
            fileURL: pathToFileURL(file),
        });
        store.set({ id, data, body, filePath, digest, rendered });
    }
    untouched.forEach((id) => store.delete(id));
}
