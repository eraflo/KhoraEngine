// Tests of `integration.mjs`: the dev server serves the local frames.
// Run with `npm test` (Node's own test runner, no dependency).

import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { FRAMES, MANIFEST } from './frames.mjs';
import { landingFrames } from './integration.mjs';

/** Returns the dev middleware the integration installs over the frames in `dir`. */
function devMiddleware(dir) {
    let config = null;
    const integration = landingFrames({ dir, base: '/KhoraEngine' });
    integration.hooks['astro:config:setup']({
        updateConfig: (update) => {
            config = update;
        },
        logger: { info: () => {} },
    });
    let middleware = null;
    for (const plugin of config.vite.plugins) {
        plugin.configureServer({ middlewares: { use: (fn) => (middleware = fn) } });
    }
    return middleware;
}

/** Asks `middleware` for `url`; returns the bytes it answered, or `'next'`. */
function ask(middleware, url) {
    let answer = 'next';
    const response = { setHeader: () => {}, end: (bytes) => (answer = Buffer.from(bytes).toString()) };
    middleware({ url }, response, () => {});
    return answer;
}

test('the dev server serves a local frame, with or without the base in its URL', (t) => {
    const dir = mkdtempSync(join(tmpdir(), 'khora-frames-'));
    t.after(() => rmSync(dir, { recursive: true, force: true }));
    const { version, format, scenes } = FRAMES;
    writeFileSync(join(dir, MANIFEST), JSON.stringify({ version, format, scenes }));
    mkdirSync(join(dir, 'chess'));
    writeFileSync(join(dir, 'chess', `frame_000.${format}`), 'frame');

    const middleware = devMiddleware(dir);

    assert.equal(ask(middleware, `/KhoraEngine/landing/frames/chess/frame_000.${format}`), 'frame');
    assert.equal(ask(middleware, `/landing/frames/chess/frame_000.${format}`), 'frame');
    assert.equal(ask(middleware, `/KhoraEngine/landing/frames/chess/frame_001.${format}`), 'next');
    assert.equal(ask(middleware, '/KhoraEngine/landing/frames/../package.json'), 'next');
    assert.equal(ask(middleware, '/KhoraEngine/'), 'next');
});

test('with no local frames, the dev server serves none', (t) => {
    const dir = mkdtempSync(join(tmpdir(), 'khora-frames-'));
    t.after(() => rmSync(dir, { recursive: true, force: true }));
    let config = null;
    landingFrames({ dir, base: '/KhoraEngine' }).hooks['astro:config:setup']({
        updateConfig: (update) => {
            config = update;
        },
        logger: { info: () => {} },
    });
    assert.deepEqual(config.vite.plugins, []);
});
