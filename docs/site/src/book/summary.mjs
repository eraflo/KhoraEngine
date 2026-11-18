// The book's table of contents, read from `docs/src/SUMMARY.md`.
//
// SUMMARY.md is the one source of the navigation: the site builds its sidebar
// from it here, at build time. Its grammar is mdBook's (the book's first
// build, retired since), the subset the book uses:
//
//   # Summary                  the book's title line, first
//   [Home](./home.md)          a chapter before the first part
//   # Tutorials                a part
//   - [Start](./tutorials/start-from-zero.md)
//     - [Child](./x/child.md)  nested by two spaces
//
// Any other line is refused, so the sidebar never silently drops an entry.

import { posix } from 'node:path';

/**
 * Parses SUMMARY.md into a tree of parts and chapters.
 *
 * @param {string} text The content of SUMMARY.md.
 * @returns {Array<{kind: 'chapter', label: string, path: string, children: any[]} | {kind: 'part', label: string, items: any[]}>}
 * @throws {Error} On a line outside the grammar, naming its number.
 */
export function parseSummary(text) {
    const tree = [];
    let part = null;
    const stack = [];
    let titleSeen = false;
    text.replace(/^\uFEFF/, '')
        .split(/\r?\n/)
        .forEach((line, index) => {
            const where = `SUMMARY.md:${index + 1}`;
            if (line.trim() === '') return;
            const heading = /^#\s+(.+?)\s*$/.exec(line);
            if (heading) {
                if (!titleSeen) {
                    titleSeen = true;
                    return;
                }
                part = { kind: 'part', label: heading[1], items: [] };
                tree.push(part);
                stack.length = 0;
                return;
            }
            const prefix = /^\[(.+)\]\((.+)\)\s*$/.exec(line);
            if (prefix && part === null) {
                tree.push(chapter(prefix[1], prefix[2], where));
                return;
            }
            const item = /^( *)- \[(.+)\]\((.+)\)\s*$/.exec(line);
            if (item && part !== null) {
                const indent = item[1].length;
                if (indent % 2 !== 0) throw new Error(`${where}: indent by two spaces`);
                const depth = indent / 2;
                if (depth > stack.length) throw new Error(`${where}: nested deeper than its parent`);
                stack.length = depth;
                const entry = chapter(item[2], item[3], where);
                (depth === 0 ? part.items : stack[depth - 1].children).push(entry);
                stack.push(entry);
                return;
            }
            throw new Error(`${where}: not a SUMMARY.md line the site understands: "${line}"`);
        });
    return tree;
}

/**
 * Lists every page path SUMMARY.md links, in reading order.
 *
 * @param {ReturnType<typeof parseSummary>} tree
 * @returns {string[]} Paths relative to `docs/src`, with `/` separators.
 */
export function summaryPaths(tree) {
    const paths = [];
    const visit = (entry) => {
        paths.push(entry.path);
        entry.children.forEach(visit);
    };
    for (const node of tree) {
        if (node.kind === 'part') node.items.forEach(visit);
        else visit(node);
    }
    return paths;
}

/**
 * Builds Starlight's `sidebar` from the parsed tree.
 *
 * A part becomes an open group; a chapter with children becomes a folded
 * group that opens on its own page first, as mdBook folded them.
 *
 * @param {ReturnType<typeof parseSummary>} tree
 * @returns {object[]} Starlight sidebar items, linked by page id.
 */
export function sidebarFromSummary(tree) {
    const item = (entry) => {
        const link = { label: entry.label, slug: pageId(entry.path) };
        if (entry.children.length === 0) return link;
        return { label: entry.label, collapsed: true, items: [link, ...entry.children.map(item)] };
    };
    return tree.map((node) =>
        node.kind === 'part'
            ? { label: node.label, collapsed: false, items: node.items.map(item) }
            : item(node),
    );
}

/**
 * Returns the site id (and route) of a page: its path without `.md`, an
 * `index` page named by its folder.
 *
 * @param {string} path A page path relative to `docs/src`, `/` separated.
 * @returns {string} The id, e.g. `concepts/scripting` for `concepts/scripting/index.md`.
 */
export function pageId(path) {
    const id = posix.normalize(path).replace(/\.md$/, '');
    if (id === 'index') return id;
    return id.replace(/\/index$/, '');
}

function chapter(label, target, where) {
    const path = posix.normalize(target.replace(/^\.\//, ''));
    if (!path.endsWith('.md')) throw new Error(`${where}: "${target}" is not a .md page`);
    return { kind: 'chapter', label, path, children: [] };
}
