// The landing page's film: the frames Khora rendered for it, and where the
// page finds them.
//
// The frames are not in the repository. They are AVIF stills, packed into one
// tar archive (`scripts/encode-frames.mjs`) that is published as an asset of a
// GitHub release; `npm run frames` fetches it, checks its SHA-256 and unpacks
// it into `docs/site/frames/`, which git ignores. A build then serves them at
// `<base>/landing/frames/`, or the page loads them from `KHORA_FRAMES_URL`
// when that is set. With neither, the page is built without its film: each
// scene shows its poster, a still kept in `src/assets/landing/`.

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

/** What the film is: its version, its format, and the frames of each scene. */
export const FRAMES = {
    /** Bumped whenever the frames are re-rendered or re-encoded. */
    version: 1,
    format: 'avif',
    /** The release that holds the archive, and the archive's SHA-256. */
    release: {
        repository: 'eraflo/KhoraEngine',
        tag: 'landing-frames-v1',
        asset: 'khora-landing-frames-v1.tar',
        sha256: '9a7e80b0c02342858aacfd235ac60c87e1b1c86e147f063192b6803bfb89a6d4',
    },
    /** Frames per scene, numbered from 0. */
    scenes: { chess: 36, valley: 24, fireflies: 30, physics: 48 },
};

/** The stills the page shows before its frames load, or instead of them: scene and frame. */
export const POSTERS = {
    chess: ['chess', 0],
    valley: ['valley', 0],
    fireflies: ['fireflies', 0],
    physics: ['physics', 0],
    'chess-end': ['chess', FRAMES.scenes.chess - 1],
};

/** The archive's download URL. */
export const RELEASE_URL = `https://github.com/${FRAMES.release.repository}/releases/download/${FRAMES.release.tag}/${FRAMES.release.asset}`;

/** The file listing the frames, at the root of the archive and of the frames folder. */
export const MANIFEST = 'manifest.json';

/** The path under the site where a build serves the frames it found locally. */
export const SERVED_PATH = 'landing/frames/';

/** The path of frame `index` of `scene`, relative to the frames' base. */
export function framePath(scene, index) {
    return `${scene}/frame_${String(index).padStart(3, '0')}.${FRAMES.format}`;
}

/**
 * Where the page finds the frames of `scene`: `base` and the frame's path,
 * `{n}` standing for its three-digit number (the film script fills it in).
 * Null when the page has no frames.
 */
export function framePattern(base, scene) {
    return base ? `${base}${scene}/frame_{n}.${FRAMES.format}` : null;
}

/** The manifest the archive carries: what `FRAMES` says, as JSON. */
export function manifest() {
    return { version: FRAMES.version, format: FRAMES.format, scenes: { ...FRAMES.scenes } };
}

/**
 * Whether `name` may be in the frames archive: the manifest, or a frame of a
 * known scene within its count. Anything else (a path climbing out, an
 * absolute path, a stray file) is refused.
 */
export function isFrameEntry(name) {
    if (name === MANIFEST) return true;
    const match = /^([a-z]+)\/frame_(\d{3})\.([a-z0-9]+)$/.exec(name);
    if (!match) return false;
    const count = FRAMES.scenes[match[1]];
    return count !== undefined && Number(match[2]) < count && match[3] === FRAMES.format;
}

/**
 * What a manifest disagrees with `FRAMES` on, one sentence each; none when it
 * describes these frames.
 */
export function manifestProblems(found) {
    const want = manifest();
    const problems = [];
    if (!found || typeof found !== 'object') return ['the manifest is not an object'];
    if (found.version !== want.version) problems.push(`version ${found.version}, the site wants ${want.version}`);
    if (found.format !== want.format) problems.push(`format ${found.format}, the site wants ${want.format}`);
    for (const [scene, count] of Object.entries(want.scenes)) {
        const has = found.scenes?.[scene];
        if (has !== count) problems.push(`${scene}: ${has ?? 'no'} frames, the site wants ${count}`);
    }
    return problems;
}

/**
 * Where the page loads its frames from, decided once per build.
 *
 * - `KHORA_FRAMES_URL` set: that URL (an `https://` URL or a path from the
 *   host's root), with a trailing `/`.
 * - Otherwise, frames unpacked in `dir` whose manifest matches: the site's own
 *   `<base>/landing/frames/`.
 * - Otherwise none: the page shows its posters.
 *
 * @param {object} options
 * @param {string | undefined} options.url The `KHORA_FRAMES_URL` value.
 * @param {string} options.dir The local frames folder.
 * @param {string} options.base The site's base path, without a trailing `/`.
 * @returns {{ from: 'url' | 'local' | 'none', base: string | null, why: string }}
 */
export function framesSource({ url, dir, base }) {
    if (url) {
        if (!/^(https:\/\/[^\s]+|\/[^\s]*)$/.test(url)) {
            throw new Error(`KHORA_FRAMES_URL must be an https:// URL or a path from the root, not ${url}`);
        }
        return { from: 'url', base: url.endsWith('/') ? url : `${url}/`, why: `frames from ${url}` };
    }
    const file = join(dir, MANIFEST);
    if (!existsSync(file)) {
        return { from: 'none', base: null, why: 'no frames in docs/site/frames (run `npm run frames`): posters only' };
    }
    let problems;
    try {
        problems = manifestProblems(JSON.parse(readFileSync(file, 'utf8')));
    } catch (error) {
        problems = [`unreadable manifest: ${error.message}`];
    }
    if (problems.length) {
        return { from: 'none', base: null, why: `the local frames do not match the site (${problems.join('; ')}): posters only` };
    }
    return { from: 'local', base: `${base}/${SERVED_PATH}`, why: 'frames from docs/site/frames' };
}
