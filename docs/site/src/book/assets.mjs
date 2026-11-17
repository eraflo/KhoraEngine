// Serves the book's files that are not pages: its logos and figures.
//
// A page links them relatively (`../logos/khora-sign.svg`); `page.mjs`
// rewrites such a link to the file's path under the site base. This
// integration mirrors every non-Markdown file of `docs/src` into `public/`
// at that same path before `astro dev` or `astro build` starts, so both
// serve it (`astro preview` serves `dist/`, already holding them). `public/`
// is generated and git ignores it: a file there that `docs/src` no longer
// holds is removed.

import { cpSync, readdirSync, rmSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Creates the integration that mirrors the book's non-page files.
 *
 * @param {object} options
 * @param {string} options.srcDir The absolute path of `docs/src`.
 * @returns {import('astro').AstroIntegration}
 */
export function bookAssets({ srcDir }) {
    return {
        name: 'khora-book-assets',
        hooks: {
            'astro:config:setup': ({ config, command, logger }) => {
                if (command === 'preview') return;
                const publicDir = fileURLToPath(config.publicDir);
                if (relative(fileURLToPath(config.root), publicDir).startsWith('..')) {
                    throw new Error(`publicDir ${publicDir} is outside the site: refusing to write it`);
                }
                const copied = mirrorBookAssets(srcDir, publicDir);
                logger.info(`mirrored ${copied} files from docs/src into public/`);
            },
        },
    };
}

/**
 * Copies into `publicDir` every file of `srcDir` that is not Markdown,
 * keeping its path, and removes from `publicDir` every file `srcDir` does not
 * hold.
 *
 * @param {string} srcDir
 * @param {string} publicDir
 * @returns {number} How many files were copied.
 */
export function mirrorBookAssets(srcDir, publicDir) {
    const wanted = new Set(assetPaths(srcDir));
    for (const path of assetPaths(publicDir)) {
        if (!wanted.has(path)) rmSync(join(publicDir, path), { force: true });
    }
    for (const path of wanted) cpSync(join(srcDir, path), join(publicDir, path));
    return wanted.size;
}

/** The non-Markdown files under `dir`, relative to it; none when it is missing. */
function assetPaths(dir) {
    let entries;
    try {
        entries = readdirSync(dir, { recursive: true, withFileTypes: true });
    } catch {
        return [];
    }
    return entries
        .filter((entry) => entry.isFile() && !entry.name.endsWith('.md'))
        .map((entry) => relative(dir, join(entry.parentPath, entry.name)));
}
