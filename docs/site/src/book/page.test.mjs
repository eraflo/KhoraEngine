// Tests of `page.mjs`: an mdBook page prepared for the site.
// Run with `npm test` (Node's own test runner, no dependency).

import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { preparePage, resolveTarget } from './page.mjs';

const BASE = '/KhoraEngine';

function prepare(text, path = 'concepts/gorna.md', srcDir = '/nowhere', label = undefined) {
    return preparePage({ text, path, srcDir, base: BASE, label });
}

test('the first heading becomes the title and leaves the body', () => {
    const page = prepare('# GORNA\r\n\r\nThe idea.\r\n\r\n# Not the title\r\n');
    assert.equal(page.title, 'GORNA');
    assert.equal(page.body, 'The idea.\n\n# Not the title\n');
});

test('a heading inside a code fence is not the title', () => {
    const page = prepare('```bash\n# a comment\n```\n\n# Real\n', 'a.md');
    assert.equal(page.title, 'Real');
});

test('a page without a heading takes its SUMMARY.md label, then its file name', () => {
    assert.equal(prepare('<div>hi</div>\n', 'home.md', '/x', 'Home').title, 'Home');
    assert.equal(prepare('<div>hi</div>\n', 'home.md').title, 'home');
});

test('a link to a page becomes its route, its anchor kept', () => {
    const page = prepare('# T\n\nSee [the frame](./the-frame.md#the-clock) and [x](../how-to/index.md).\n');
    assert.match(page.body, /\(\/KhoraEngine\/concepts\/the-frame\/#the-clock\)/);
    assert.match(page.body, /\(\/KhoraEngine\/how-to\/\)/);
});

test('a raw HTML link to a rendered page becomes its route', () => {
    const page = prepare('<a class="kp-btn" href="tutorials/start-from-zero.html">Go</a>\n', 'home.md');
    assert.match(page.body, /href="\/KhoraEngine\/tutorials\/start-from-zero\/"/);
});

test('a link to another file of the book becomes its served URL', () => {
    const page = prepare('# T\n\n<img src="../logos/khora-sign.svg">\n\n![sign](../logos/khora-icon.svg)\n', 'design/editor.md');
    assert.match(page.body, /src="\/KhoraEngine\/logos\/khora-sign\.svg"/);
    assert.match(page.body, /\(\/KhoraEngine\/logos\/khora-icon\.svg\)/);
});

test('absolute links, anchors and links outside the book are left alone', () => {
    assert.equal(resolveTarget('https://example.com/a.md', 'a.md', BASE), 'https://example.com/a.md');
    assert.equal(resolveTarget('#here', 'a.md', BASE), '#here');
    assert.equal(resolveTarget('/KhoraEngine/api/', 'a.md', BASE), '/KhoraEngine/api/');
    assert.equal(resolveTarget('../../README.md', 'concepts/a.md', BASE), '../../README.md');
});

test('links in code fences and code spans are left as written', () => {
    const text = '# T\n\n```md\n[a](./b.md)\n```\n\nUse `[a](./b.md)` or ``[c](./d.md)``, not [e](./f.md).\n';
    const body = prepare(text, 'x.md').body;
    assert.match(body, /```md\n\[a\]\(\.\/b\.md\)\n```/);
    assert.match(body, /`\[a\]\(\.\/b\.md\)`/);
    assert.match(body, /``\[c\]\(\.\/d\.md\)``/);
    assert.match(body, /\[e\]\(\/KhoraEngine\/f\/\)/);
});

test("a fence's rustdoc attributes are dropped, its language kept", () => {
    const body = prepare('# T\n\n```rust,ignore\nfn a() {}\n```\n', 'x.md').body;
    assert.match(body, /^```rust$/m);
});

test('an include is replaced by the file, relative to the page', () => {
    const src = mkdtempSync(join(tmpdir(), 'khora-book-'));
    mkdirSync(join(src, 'images', 'gorna'), { recursive: true });
    mkdirSync(join(src, 'concepts'));
    writeFileSync(join(src, 'images', 'gorna', 'share.svg'), '\uFEFF<svg id="fig-gorna-share"></svg>\n');
    const page = prepare('# T\n\n<div>\n\n{{#include ../images/gorna/share.svg}}\n\n</div>\n', 'concepts/gorna.md', src);
    assert.match(page.body, /\n<svg id="fig-gorna-share"><\/svg>\n/);
});

test('an include of a file outside the book is refused', () => {
    assert.throws(() => prepare('{{#include ../../../.env}}', 'concepts/a.md', tmpdir()), /outside the book/);
    assert.throws(() => prepare('{{#include ../../secret.txt}}', 'a.md', tmpdir()), /outside the book/);
});

test('an include of a missing file, or of a range, is refused', () => {
    assert.throws(() => prepare('{{#include ../images/none.svg}}', 'concepts/a.md', tmpdir()), /none\.svg/);
    assert.throws(() => prepare('{{#include ../src/lib.rs:10:20}}', 'concepts/a.md'), /range or anchor/);
});
