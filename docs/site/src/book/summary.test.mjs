// Tests of `summary.mjs`: SUMMARY.md read into the site's sidebar.
// Run with `npm test` (Node's own test runner, no dependency).

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import { pageId, parseSummary, sidebarFromSummary, summaryPaths } from './summary.mjs';

const SAMPLE = [
    '# Summary',
    '',
    '[Home](./home.md)',
    '',
    '# Concepts',
    '- [Overview](./concepts/index.md)',
    '- [Data](./concepts/ecs.md)',
    '  - [Changes](./concepts/change-detection.md)',
    '- [Scripting](./concepts/scripting/index.md)',
    '',
    '# Reference',
    '- [FAQ](./faq.md)',
].join('\r\n');

test('a chapter before the first part stays at the top, a part becomes an open group', () => {
    const sidebar = sidebarFromSummary(parseSummary(SAMPLE));
    assert.deepEqual(sidebar[0], { label: 'Home', slug: 'home' });
    assert.equal(sidebar[1].label, 'Concepts');
    assert.equal(sidebar[1].collapsed, false);
    assert.deepEqual(sidebar[2], { label: 'Reference', collapsed: false, items: [{ label: 'FAQ', slug: 'faq' }] });
});

test('a chapter with children becomes a folded group that opens on its own page', () => {
    const concepts = sidebarFromSummary(parseSummary(SAMPLE))[1];
    assert.deepEqual(concepts.items[1], {
        label: 'Data',
        collapsed: true,
        items: [
            { label: 'Data', slug: 'concepts/ecs' },
            { label: 'Changes', slug: 'concepts/change-detection' },
        ],
    });
});

test('summary paths list every page in reading order', () => {
    assert.deepEqual(summaryPaths(parseSummary(SAMPLE)), [
        'home.md',
        'concepts/index.md',
        'concepts/ecs.md',
        'concepts/change-detection.md',
        'concepts/scripting/index.md',
        'faq.md',
    ]);
});

test('an index page is named by its folder', () => {
    assert.equal(pageId('concepts/scripting/index.md'), 'concepts/scripting');
    assert.equal(pageId('concepts/index.md'), 'concepts');
    assert.equal(pageId('./faq.md'), 'faq');
    assert.equal(pageId('index.md'), 'index');
});

test('a line outside the grammar is refused with its number', () => {
    assert.throws(() => parseSummary('# Summary\n\n# Part\n* [Star](./a.md)\n'), /SUMMARY\.md:4/);
    assert.throws(() => parseSummary('# Summary\n\n# Part\n- [A](./a.md)\n   - [B](./b.md)\n'), /two spaces/);
    assert.throws(() => parseSummary('# Summary\n\n# Part\n  - [A](./a.md)\n'), /deeper/);
    assert.throws(() => parseSummary('# Summary\n\n# Part\n- [A](./a.html)\n'), /not a \.md page/);
});

test("the book's own SUMMARY.md parses and links only existing pages", () => {
    const text = readFileSync(new URL('../../../src/SUMMARY.md', import.meta.url), 'utf8');
    for (const path of summaryPaths(parseSummary(text))) {
        assert.doesNotThrow(() => readFileSync(new URL(`../../../src/${path}`, import.meta.url)), path);
    }
});
