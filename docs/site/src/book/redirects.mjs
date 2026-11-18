// The book's old URLs, kept working: every page mdBook served as `x.html`
// now lives at its route `x/`.
//
// The site is hosted statically (GitHub Pages), with no server rule to
// redirect by, so each old path gets a small page of its own, written into
// the build at that exact path: it sends the reader on to the route, keeping
// the `#anchor`, and names the route as canonical. Astro's own `redirects`
// would write `x.html/index.html` in the site's directory format, not the
// `x.html` file an old link asks for.
//
// The list is computed from SUMMARY.md on every build, so a page added there
// has its old URL redirected with no further step. A page `x/index.md` needs
// none: mdBook served it at `x/index.html`, where the site serves its route.

import { mkdirSync, existsSync, writeFileSync } from 'node:fs';
import { dirname, join, posix } from 'node:path';
import { fileURLToPath } from 'node:url';

import { SITE } from './site.mjs';
import { pageId, summaryPaths } from './summary.mjs';

/**
 * The redirects mdBook itself served (`[output.html.redirect]` of the
 * retired `book.toml`): an old path, and the page it now leads to.
 */
export const BOOK_REDIRECTS = [
    ['open_questions.html', 'open-questions.md'],
    ['concepts/scripting.html', 'concepts/scripting/index.md'],
];

/**
 * The URL path mdBook served a page at, relative to the base.
 *
 * @param {string} path A page path relative to `docs/src`, `/` separated.
 * @returns {string} e.g. `concepts/gorna.html`.
 */
export function oldPath(path) {
    return posix.normalize(path).replace(/\.md$/, '.html');
}

/**
 * The route the site serves a page at, under the base.
 *
 * @param {string} path A page path relative to `docs/src`.
 * @param {string} base The site base, e.g. `/KhoraEngine` (no trailing `/`).
 * @returns {string} e.g. `/KhoraEngine/concepts/gorna/`.
 */
export function route(path, base) {
    const id = pageId(path);
    return `${base}/${id === 'index' ? '' : `${id}/`}`;
}

/**
 * Whether the site already serves the page at its old path: an `index.md`
 * page, whose old `x/index.html` is the file its route is built to.
 *
 * @param {string} path A page path relative to `docs/src`.
 */
export function servedAtOldPath(path) {
    return posix.basename(posix.normalize(path)) === 'index.md';
}

/**
 * Every redirect the build writes: one per page of SUMMARY.md whose old path
 * the site does not serve itself, and those of {@link BOOK_REDIRECTS}.
 *
 * @param {ReturnType<import('./summary.mjs').parseSummary>} summary
 * @param {string} base The site base, no trailing `/`.
 * @returns {Array<{from: string, to: string}>} `from` relative to the base, `to` absolute.
 * @throws {Error} When two redirects claim one old path.
 */
export function redirectsFromSummary(summary, base) {
    const pages = summaryPaths(summary);
    const listed = new Set(pages);
    const redirects = pages
        .filter((path) => !servedAtOldPath(path))
        .map((path) => ({ from: oldPath(path), to: route(path, base) }));
    for (const [from, page] of BOOK_REDIRECTS) {
        if (!listed.has(page)) throw new Error(`the redirect from ${from} leads to ${page}, not in SUMMARY.md`);
        redirects.push({ from, to: route(page, base) });
    }
    const seen = new Set();
    for (const { from } of redirects) {
        if (seen.has(from)) throw new Error(`two redirects from ${from}`);
        seen.add(from);
    }
    return redirects;
}

/**
 * The page written at an old path: it leads to `to` at once, keeping the
 * anchor; without scripts, the refresh leads there too.
 *
 * @param {string} to The route, absolute (`/KhoraEngine/concepts/gorna/`).
 * @returns {string} The HTML.
 */
export function redirectPage(to) {
    const href = escapeHtml(to);
    return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Moved | Khora</title>
<meta name="robots" content="noindex">
<link rel="canonical" href="${escapeHtml(SITE + to)}">
<script>location.replace(${JSON.stringify(to)} + location.hash)</script>
<meta http-equiv="refresh" content="0; url=${href}">
</head>
<body>
<p>This page moved to <a href="${href}">${href}</a>.</p>
</body>
</html>
`;
}

/**
 * Writes the redirect pages into a built site.
 *
 * @param {string} outDir The built site's folder (`dist/`).
 * @param {Array<{from: string, to: string}>} redirects
 * @returns {number} How many were written.
 * @throws {Error} When a redirect would replace a file the build made, or
 *   leave `outDir`.
 */
export function writeRedirects(outDir, redirects) {
    for (const { from } of redirects) {
        if (from.startsWith('/') || posix.normalize(from).startsWith('..')) {
            throw new Error(`the redirect from ${from} leaves the site`);
        }
        if (existsSync(join(outDir, from))) {
            throw new Error(`the redirect from ${from} would replace a page the build made`);
        }
    }
    for (const { from, to } of redirects) {
        const file = join(outDir, from);
        mkdirSync(dirname(file), { recursive: true });
        writeFileSync(file, redirectPage(to));
    }
    return redirects.length;
}

/**
 * Creates the integration that writes the redirects once the site is built.
 *
 * @param {object} options
 * @param {ReturnType<import('./summary.mjs').parseSummary>} options.summary
 * @param {string} options.base The site base, no trailing `/`.
 * @returns {import('astro').AstroIntegration}
 */
export function bookRedirects({ summary, base }) {
    return {
        name: 'khora-book-redirects',
        hooks: {
            'astro:build:done': ({ dir, logger }) => {
                const written = writeRedirects(fileURLToPath(dir), redirectsFromSummary(summary, base));
                logger.info(`wrote ${written} redirects from the book's old .html URLs`);
            },
        },
    };
}

function escapeHtml(text) {
    return text.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
