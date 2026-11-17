// Turns one book page, written for mdBook, into the Markdown the site renders.
//
// The pages stay mdBook pages in `docs/src`: no frontmatter, a `# Title` on
// their first line, figures inlined with `{{#include}}`, links to `x.md`
// (Markdown) or `x.html` (raw HTML). This module reads them as they are, in
// memory, so neither the pages nor the paths that code comments and the
// xtask checks cite ever change:
//
// - `{{#include path}}` is replaced by the file's text, as mdBook does;
// - the first `# Title` becomes the page's title (Starlight draws it) and
//   leaves the body; a page without one takes its SUMMARY.md label;
// - a relative link to a page becomes the page's route under the site base;
//   one to any other file under `docs/src` becomes that file's URL, the
//   file being copied to `public/` by `assets.mjs`;
// - a code fence's rustdoc attributes (`rust,ignore`) are dropped, leaving
//   the language the highlighter knows.
//
// Fenced code and inline code spans are left as written, except for
// `{{#include}}`, which mdBook also expands everywhere.

import { readFileSync } from 'node:fs';
import { dirname, isAbsolute, join, posix, relative } from 'node:path';

import { pageId } from './summary.mjs';

const INCLUDE = /\{\{#include\s+([^}\s]+)\s*\}\}/g;
const MARKDOWN_LINK = /(\]\(\s*<?)([^)\s>]+)(>?(?:\s+"[^"]*")?\s*\))/g;
const REFERENCE_DEFINITION = /^(\s{0,3}\[[^\]]+\]:\s*<?)([^\s>]+)(.*)$/;
const FENCE_ATTRIBUTES = /^(\s{0,3}(?:`{3,}|~{3,})\s*[\w+-]+),[\w,-]*/;
const HTML_ATTRIBUTE = /(\b(?:href|src)\s*=\s*)("([^"]*)"|'([^']*)')/g;

/**
 * Prepares a page for the site.
 *
 * @param {object} page
 * @param {string} page.text The page as written.
 * @param {string} page.path Its path relative to `docs/src`, `/` separated.
 * @param {string} page.srcDir The absolute path of `docs/src`.
 * @param {string} page.base The site base, e.g. `/KhoraEngine` (no trailing `/`).
 * @param {string | undefined} page.label Its SUMMARY.md label, the title of last resort.
 * @returns {{title: string, body: string}}
 * @throws {Error} When an include names a file that cannot be read, or a
 *   line range or anchor (`file:3:9`), which the site does not support.
 */
export function preparePage({ text, path, srcDir, base, label }) {
    const expanded = expandIncludes(text.replace(/^\uFEFF/, ''), path, srcDir).replace(/\r\n?/g, '\n');
    const { title, body } = takeTitle(expanded);
    const pageTitle = title ?? label ?? posix.basename(path, '.md');
    return { title: pageTitle, body: rewriteLinks(body, path, base) };
}

/**
 * Resolves one link target written in the page at `path`.
 *
 * @param {string} target The target as written (`../concepts/gorna.md#the-quote`).
 * @param {string} path The page's path relative to `docs/src`.
 * @param {string} base The site base, no trailing `/`.
 * @returns {string} The URL the site serves it at; the target itself when it
 *   is absolute, an anchor, or outside `docs/src`.
 */
export function resolveTarget(target, path, base) {
    if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('#') || target.startsWith('/')) {
        return target;
    }
    const hash = target.indexOf('#');
    const file = hash < 0 ? target : target.slice(0, hash);
    const fragment = hash < 0 ? '' : target.slice(hash);
    if (file === '') return target;
    const resolved = posix.normalize(posix.join(posix.dirname(path), file));
    if (resolved.startsWith('../') || resolved === '..') return target;
    if (/\.(md|html)$/.test(resolved)) {
        const id = pageId(resolved.replace(/\.html$/, '.md'));
        return `${base}/${id === 'index' ? '' : `${id}/`}${fragment}`;
    }
    return `${base}/${resolved}${fragment}`;
}

function expandIncludes(text, path, srcDir) {
    return text.replace(INCLUDE, (_, target) => {
        if (/:[^/\\]*$/.test(target) && !/^[a-z]:[/\\]/i.test(target)) {
            throw new Error(`${path}: {{#include ${target}}} names a range or anchor; include whole files`);
        }
        const file = join(srcDir, dirname(path), target);
        // A page includes only files of the book: never one beside it, or an absolute path.
        const inBook = relative(srcDir, file);
        if (isAbsolute(target) || inBook.startsWith('..') || isAbsolute(inBook)) {
            throw new Error(`${path}: {{#include ${target}}} names a file outside the book`);
        }
        try {
            return readFileSync(file, 'utf8').replace(/^\uFEFF/, '').trimEnd();
        } catch (error) {
            throw new Error(`${path}: {{#include ${target}}}: ${error.message}`);
        }
    });
}

function takeTitle(text) {
    const lines = text.split('\n');
    let fence = null;
    for (let i = 0; i < lines.length; i++) {
        fence = nextFence(fence, lines[i]);
        if (fence !== null || isFenceLine(lines[i])) continue;
        const heading = /^#\s+(.+?)\s*#*\s*$/.exec(lines[i]);
        if (heading) {
            const drop = lines[i + 1]?.trim() === '' ? 2 : 1;
            lines.splice(i, drop);
            return { title: heading[1].replace(/\r$/, ''), body: lines.join('\n') };
        }
    }
    return { title: undefined, body: text };
}

function rewriteLinks(text, path, base) {
    let fence = null;
    return text
        .split('\n')
        .map((line) => {
            const opens = fence === null;
            fence = nextFence(fence, line);
            if (isFenceLine(line)) return opens && fence !== null ? line.replace(FENCE_ATTRIBUTES, '$1') : line;
            if (fence !== null) return line;
            const definition = REFERENCE_DEFINITION.exec(line);
            if (definition) {
                return definition[1] + resolveTarget(definition[2], path, base) + definition[3];
            }
            return outsideCodeSpans(line, (segment) =>
                segment
                    .replace(MARKDOWN_LINK, (_, open, target, close) => open + resolveTarget(target, path, base) + close)
                    .replace(HTML_ATTRIBUTE, (_, name, quoted, double, single) => {
                        const value = double ?? single;
                        const quote = double === undefined ? "'" : '"';
                        return `${name}${quote}${resolveTarget(value, path, base)}${quote}`;
                    }),
            );
        })
        .join('\n');
}

/**
 * Returns the fence open after `line`: `open` (the marker of the fence the
 * line is in, or null) updated by the line opening or closing one.
 */
function nextFence(open, line) {
    const match = /^\s{0,3}(`{3,}|~{3,})(.*)$/.exec(line);
    if (!match) return open;
    if (open === null) return match[1];
    const closes = match[1][0] === open[0] && match[1].length >= open.length && match[2].trim() === '';
    return closes ? null : open;
}

/** Tells whether `line` is a fence line, opening or closing. */
function isFenceLine(line) {
    return /^\s{0,3}(`{3,}|~{3,})/.test(line);
}

/**
 * Applies `rewrite` to the parts of `line` outside inline code spans: a run
 * of backticks opens a span that the next run of the same length closes.
 */
function outsideCodeSpans(line, rewrite) {
    let out = '';
    let plain = 0;
    let i = 0;
    while (i < line.length) {
        if (line[i] !== '`') {
            i++;
            continue;
        }
        let run = i;
        while (run < line.length && line[run] === '`') run++;
        const ticks = line.slice(i, run);
        let close = line.indexOf(ticks, run);
        while (close >= 0 && (line[close + ticks.length] === '`' || line[close - 1] === '`')) {
            let end = close;
            while (line[end] === '`') end++;
            close = line.indexOf(ticks, end);
        }
        if (close < 0) {
            i = run;
            continue;
        }
        out += rewrite(line.slice(plain, i)) + line.slice(i, close + ticks.length);
        i = close + ticks.length;
        plain = i;
    }
    return out + rewrite(line.slice(plain));
}
