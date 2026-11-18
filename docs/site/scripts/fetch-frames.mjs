// Fetches the landing's frames into `docs/site/frames/`, where a build finds
// them.
//
//   npm run frames                       (the release `FRAMES.release` names)
//   npm run frames -- --from <file|url>  (an archive elsewhere: a local file, or a mirror)
//   npm run frames -- --if-released      (CI: a release not published yet is
//                                         no error; the site builds with its posters)
//
// The archive is checked against `FRAMES.release.sha256` before a byte of it
// is unpacked; every entry must be the manifest or a frame `isFrameEntry`
// knows. Anything else fails the whole fetch, and the folder is left as it was.

import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { FRAMES, MANIFEST, RELEASE_URL, isFrameEntry, manifestProblems } from '../src/landing/frames.mjs';
import { unpackTar } from '../src/landing/tar.mjs';

/** A fetch refused for a reason the reader can act on. */
class Refused extends Error {}

const framesDir = fileURLToPath(new URL('../frames/', import.meta.url));

const { values } = parseArgs({
    options: { from: { type: 'string', default: RELEASE_URL }, 'if-released': { type: 'boolean', default: false } },
});
const from = values.from;
const ifReleased = values['if-released'];

try {
    await main();
} catch (error) {
    if (!(error instanceof Refused)) throw error;
    console.error(`fetch-frames: ${error.message}`);
    // Not process.exit(): exiting while a download's handles close aborts Node on Windows.
    process.exitCode = 1;
}

async function main() {
    const archive = /^https:\/\//.test(from) ? await download(from) : readFileSync(from);
    if (archive === null) return;
    const sha256 = createHash('sha256').update(archive).digest('hex');
    if (sha256 !== FRAMES.release.sha256) {
        fail(`${from} has SHA-256 ${sha256}; the site expects ${FRAMES.release.sha256}`);
    }

    let entries;
    try {
        entries = unpackTar(archive);
    } catch (error) {
        fail(`${from}: ${error.message}`);
    }
    const refused = entries.filter((entry) => !isFrameEntry(entry.name));
    if (refused.length) fail(`${from} holds files that are not frames: ${refused.map((e) => e.name).join(', ')}`);
    const manifestEntry = entries.find((entry) => entry.name === MANIFEST);
    if (!manifestEntry) fail(`${from} has no ${MANIFEST}`);
    const problems = manifestProblems(JSON.parse(manifestEntry.data.toString('utf8')));
    if (problems.length) fail(`${from}: ${problems.join('; ')}`);
    const frames = Object.values(FRAMES.scenes).reduce((sum, count) => sum + count, 0);
    if (entries.length !== frames + 1) fail(`${from} holds ${entries.length - 1} frames, the site wants ${frames}`);

    rmSync(framesDir, { recursive: true, force: true });
    for (const { name, data } of entries) {
        const path = join(framesDir, name);
        mkdirSync(dirname(path), { recursive: true });
        writeFileSync(path, data);
    }
    console.log(`fetch-frames: ${frames} frames (v${FRAMES.version}) unpacked into ${framesDir}`);
}

/** The archive at `url`; null when it is not published and `--if-released` was given. */
async function download(url) {
    console.log(`fetch-frames: downloading ${url}`);
    let response;
    try {
        response = await fetch(url, { redirect: 'follow' });
    } catch (error) {
        fail(`could not reach ${url}: ${error.message}`);
    }
    // Only a missing archive is forgiven: one that is there but wrong, or a
    // host that cannot be reached, still fails.
    if (response.status === 404 && ifReleased) {
        console.warn(`fetch-frames: ${url} is not published yet; the landing keeps its posters`);
        return null;
    }
    if (!response.ok) {
        fail(`${url} answered ${response.status}: is the release published? (see .agent/engine/reference/documentation.md)`);
    }
    return Buffer.from(await response.arrayBuffer());
}

function fail(message) {
    throw new Refused(message);
}
