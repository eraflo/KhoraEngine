// Tests of `assets.mjs`: the book's non-page files mirrored into `public/`.
// Run with `npm test` (Node's own test runner, no dependency).

import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { mirrorBookAssets } from './assets.mjs';

test('every non-Markdown file is copied at its path, and a stale one is removed', (t) => {
    const root = mkdtempSync(join(tmpdir(), 'khora-assets-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const src = join(root, 'src');
    const out = join(root, 'public');
    mkdirSync(join(src, 'logos'), { recursive: true });
    mkdirSync(join(out, 'old'), { recursive: true });
    writeFileSync(join(src, 'logos', 'sign.svg'), '<svg/>');
    writeFileSync(join(src, 'page.md'), '# Page');
    writeFileSync(join(out, 'old', 'gone.svg'), '<svg/>');

    assert.equal(mirrorBookAssets(src, out), 1);
    assert.equal(readFileSync(join(out, 'logos', 'sign.svg'), 'utf8'), '<svg/>');
    assert.equal(existsSync(join(out, 'page.md')), false);
    assert.equal(existsSync(join(out, 'old', 'gone.svg')), false);
});
