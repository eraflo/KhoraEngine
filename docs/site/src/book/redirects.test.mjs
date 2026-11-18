// Tests of `redirects.mjs`: the book's old `.html` URLs lead to the site's routes.
// Run with `npm test` (Node's own test runner, no dependency).

import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { listPages } from './loader.mjs';
import {
    BOOK_REDIRECTS,
    oldPath,
    redirectPage,
    redirectsFromSummary,
    route,
    servedAtOldPath,
    writeRedirects,
} from './redirects.mjs';
import { BASE } from './site.mjs';
import { parseSummary } from './summary.mjs';

const BOOK = fileURLToPath(new URL('../../../src/', import.meta.url));
const SUMMARY = parseSummary(readFileSync(join(BOOK, 'SUMMARY.md'), 'utf8'));

test("every page of the book has its old URL redirected, or is served there", () => {
    const redirects = new Map(redirectsFromSummary(SUMMARY, BASE).map(({ from, to }) => [from, to]));
    const pages = listPages(BOOK);
    assert.ok(pages.length > 50, `only ${pages.length} pages found under docs/src`);
    for (const page of pages) {
        if (servedAtOldPath(page)) {
            assert.equal(`${BASE}/${oldPath(page)}`, `${route(page, BASE)}index.html`, page);
            continue;
        }
        assert.equal(redirects.get(oldPath(page)), route(page, BASE), `${page}: ${oldPath(page)} is not redirected`);
    }
});

test("the redirects mdBook served are kept", () => {
    const redirects = new Map(redirectsFromSummary(SUMMARY, BASE).map(({ from, to }) => [from, to]));
    assert.equal(redirects.get('open_questions.html'), `${BASE}/open-questions/`);
    assert.equal(redirects.get('concepts/scripting.html'), `${BASE}/concepts/scripting/`);
    assert.equal(BOOK_REDIRECTS.length, 2);
});

test('an old path is the page with .html, its route the folder', () => {
    assert.equal(oldPath('concepts/gorna.md'), 'concepts/gorna.html');
    assert.equal(route('concepts/gorna.md', '/B'), '/B/concepts/gorna/');
    assert.equal(route('concepts/scripting/index.md', '/B'), '/B/concepts/scripting/');
    assert.ok(servedAtOldPath('concepts/index.md'));
    assert.ok(!servedAtOldPath('home.md'));
});

test('a redirect claimed twice, or leading off the summary, is refused', () => {
    const twice = parseSummary(
        '# Summary\n\n# P\n- [A](./open_questions.md)\n- [B](./open-questions.md)\n- [C](./concepts/scripting/index.md)\n',
    );
    assert.throws(() => redirectsFromSummary(twice, '/B'), /two redirects from open_questions\.html/);
    const missing = parseSummary('# Summary\n\n# P\n- [A](./a.md)\n');
    assert.throws(() => redirectsFromSummary(missing, '/B'), /not in SUMMARY\.md/);
});

test('the redirect page leads to its route, keeping the anchor', () => {
    const html = redirectPage('/KhoraEngine/concepts/gorna/');
    assert.match(html, /location\.replace\("\/KhoraEngine\/concepts\/gorna\/" \+ location\.hash\)/);
    assert.match(html, /<meta http-equiv="refresh" content="0; url=\/KhoraEngine\/concepts\/gorna\/">/);
    assert.match(html, /<link rel="canonical" href="https:\/\/eraflo\.github\.io\/KhoraEngine\/concepts\/gorna\/">/);
    assert.match(html, /noindex/);
});

test('the redirects are written at their old paths, never over a built page', () => {
    const dir = mkdtempSync(join(tmpdir(), 'khora-redirects-'));
    try {
        assert.equal(writeRedirects(dir, [{ from: 'concepts/gorna.html', to: '/B/concepts/gorna/' }]), 1);
        assert.match(readFileSync(join(dir, 'concepts/gorna.html'), 'utf8'), /\/B\/concepts\/gorna\//);
        mkdirSync(join(dir, 'x'), { recursive: true });
        writeFileSync(join(dir, 'x/index.html'), 'built');
        assert.throws(() => writeRedirects(dir, [{ from: 'x/index.html', to: '/B/x/' }]), /would replace/);
        assert.equal(readFileSync(join(dir, 'x/index.html'), 'utf8'), 'built');
        assert.throws(() => writeRedirects(dir, [{ from: '../out.html', to: '/B/' }]), /leaves the site/);
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
});

test('the site config writes the redirects', () => {
    const config = readFileSync(new URL('../../astro.config.mjs', import.meta.url), 'utf8');
    assert.match(config, /bookRedirects\(\{ summary: SUMMARY, base: BASE \}\)/);
});
