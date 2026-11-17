import { strict as assert } from 'node:assert';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { FRAMES, MANIFEST, framePath, framePattern, framesSource, isFrameEntry, manifest, manifestProblems } from './frames.mjs';

const BASE = '/KhoraEngine';

function folder(contents) {
    const dir = mkdtempSync(join(tmpdir(), 'khora-frames-'));
    if (contents !== undefined) writeFileSync(join(dir, MANIFEST), contents);
    return dir;
}

test('a frame path is the scene, the three-digit number and the format', () => {
    assert.equal(framePath('chess', 7), `chess/frame_007.${FRAMES.format}`);
    assert.equal(framePattern('/x/', 'valley'), `/x/valley/frame_{n}.${FRAMES.format}`);
    assert.equal(framePattern(null, 'valley'), null);
});

test('the archive holds only the manifest and the frames of known scenes', () => {
    assert.ok(isFrameEntry(MANIFEST));
    assert.ok(isFrameEntry(framePath('chess', 0)));
    assert.ok(isFrameEntry(framePath('chess', FRAMES.scenes.chess - 1)));
    for (const name of [
        framePath('chess', FRAMES.scenes.chess),
        'chess/frame_000.jpg',
        'unknown/frame_000.avif',
        '../chess/frame_000.avif',
        '/chess/frame_000.avif',
        'chess/../../evil.avif',
        'chess\\frame_000.avif',
        'readme.txt',
    ]) {
        assert.ok(!isFrameEntry(name), name);
    }
});

test('a manifest written from FRAMES matches it; one that differs says how', () => {
    assert.deepEqual(manifestProblems(manifest()), []);
    const old = { ...manifest(), version: FRAMES.version - 1, scenes: { ...FRAMES.scenes, chess: 3 } };
    const problems = manifestProblems(old);
    assert.equal(problems.length, 2);
    assert.match(problems.join(' '), /version/);
    assert.match(problems.join(' '), /chess: 3 frames/);
    assert.deepEqual(manifestProblems(null), ['the manifest is not an object']);
});

test('KHORA_FRAMES_URL wins, and ends with a slash', () => {
    const dir = folder();
    try {
        assert.deepEqual(framesSource({ url: 'https://example.org/frames', dir, base: BASE }).base, 'https://example.org/frames/');
        assert.equal(framesSource({ url: '/mirror/', dir, base: BASE }).base, '/mirror/');
        assert.throws(() => framesSource({ url: 'http://plain.example/', dir, base: BASE }), /https/);
        assert.throws(() => framesSource({ url: 'frames/', dir, base: BASE }), /https/);
    } finally {
        rmSync(dir, { recursive: true });
    }
});

test('local frames are used only when their manifest matches', () => {
    const none = folder();
    const good = folder(JSON.stringify(manifest()));
    const stale = folder(JSON.stringify({ ...manifest(), version: FRAMES.version + 1 }));
    const broken = folder('{ not json');
    try {
        assert.deepEqual(framesSource({ url: undefined, dir: none, base: BASE }).from, 'none');
        const found = framesSource({ url: '', dir: good, base: BASE });
        assert.equal(found.from, 'local');
        assert.equal(found.base, `${BASE}/landing/frames/`);
        assert.equal(framesSource({ url: undefined, dir: stale, base: BASE }).from, 'none');
        assert.equal(framesSource({ url: undefined, dir: broken, base: BASE }).from, 'none');
    } finally {
        for (const dir of [none, good, stale, broken]) rmSync(dir, { recursive: true });
    }
});

test('the release is pinned by its SHA-256', () => {
    assert.match(FRAMES.release.sha256, /^[0-9a-f]{64}$/);
});
