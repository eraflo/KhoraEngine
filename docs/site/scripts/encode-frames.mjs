// Encodes the landing's rendered frames for the site, and packs them for the
// release that holds them.
//
//   node scripts/encode-frames.mjs <renders> [--quality 45] [--posters]
//
// `<renders>` holds one folder per scene (`chess/`, `valley/`, …) of the
// frames as Khora rendered them, `frame_000.jpg` (or `.png`) on. The script:
//
// 1. encodes every frame to AVIF into `frames/<scene>/frame_NNN.avif`, the
//    folder the site serves (git ignores it), and writes its manifest;
// 2. packs that folder into `release/<asset>.tar` (git ignores it too), the
//    file to attach to the release `FRAMES.release` names, and prints its
//    SHA-256 for `FRAMES.release.sha256`;
// 3. with `--posters`, writes the stills `POSTERS` lists into
//    `src/assets/landing/` as AVIF: the only images of the film kept in git,
//    which the build turns into the sizes and formats a reader needs.
//
// Run it when the scenes are rendered again; bump `FRAMES.version` and the
// release tag with it.

import { createHash } from 'node:crypto';
import { mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import sharp from 'sharp';

import { FRAMES, MANIFEST, POSTERS, framePath, manifest } from '../src/landing/frames.mjs';
import { packTar } from '../src/landing/tar.mjs';

const site = fileURLToPath(new URL('../', import.meta.url));
const framesDir = join(site, 'frames');
const releaseDir = join(site, 'release');
const postersDir = join(site, 'src', 'assets', 'landing');

const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
        quality: { type: 'string', default: '45' },
        posters: { type: 'boolean', default: false },
    },
});
const renders = positionals[0];
if (!renders) fail('usage: node scripts/encode-frames.mjs <renders> [--quality 45] [--posters]');
const quality = Number(values.quality);
if (!Number.isInteger(quality) || quality < 1 || quality > 100) fail(`--quality must be 1 to 100, not ${values.quality}`);

const sources = {};
for (const [scene, count] of Object.entries(FRAMES.scenes)) {
    const files = frameFiles(join(renders, scene));
    if (files.length !== count) {
        fail(`${scene}: ${files.length} frames in ${join(renders, scene)}, FRAMES.scenes says ${count}`);
    }
    sources[scene] = files;
}

rmSync(framesDir, { recursive: true, force: true });
let before = 0;
let after = 0;
const entries = [];
for (const [scene, files] of Object.entries(sources)) {
    mkdirSync(join(framesDir, scene), { recursive: true });
    let sceneAfter = 0;
    for (const [index, file] of files.entries()) {
        before += statSync(file).size;
        const data = await sharp(file).avif({ quality, effort: 6, chromaSubsampling: '4:2:0' }).toBuffer();
        const name = framePath(scene, index);
        writeFileSync(join(framesDir, name), data);
        entries.push({ name, data });
        sceneAfter += data.length;
    }
    after += sceneAfter;
    console.log(`${scene}: ${files.length} frames, ${mib(sceneAfter)}`);
}
const manifestData = Buffer.from(JSON.stringify(manifest(), null, 2) + '\n');
writeFileSync(join(framesDir, MANIFEST), manifestData);
entries.unshift({ name: MANIFEST, data: manifestData });

mkdirSync(releaseDir, { recursive: true });
const archive = packTar(entries);
const archivePath = join(releaseDir, FRAMES.release.asset);
writeFileSync(archivePath, archive);
const sha256 = createHash('sha256').update(archive).digest('hex');

console.log(`\n${entries.length - 1} frames: ${mib(before)} rendered, ${mib(after)} as AVIF (quality ${quality})`);
console.log(`archive: ${archivePath} (${mib(archive.length)})`);
console.log(`sha256:  ${sha256}`);
if (sha256 !== FRAMES.release.sha256) {
    console.log('FRAMES.release.sha256 in src/landing/frames.mjs differs: set it to the sha256 above.');
}

if (values.posters) {
    mkdirSync(postersDir, { recursive: true });
    for (const [name, [scene, index]] of Object.entries(POSTERS)) {
        const out = join(postersDir, `${name}.avif`);
        await sharp(sources[scene][index]).avif({ quality: 50, effort: 6, chromaSubsampling: '4:2:0' }).toFile(out);
        console.log(`poster ${name}: ${mib(statSync(out).size)}`);
    }
}

/** The frames of one scene, in order: `frame_000.jpg`, `frame_001.jpg`, … */
function frameFiles(dir) {
    let names;
    try {
        names = readdirSync(dir);
    } catch {
        return [];
    }
    return names
        .filter((name) => /^frame_\d{3}\.(jpg|jpeg|png)$/.test(name))
        .sort()
        .map((name) => join(dir, name));
}

function mib(bytes) {
    return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
}

function fail(message) {
    console.error(`encode-frames: ${message}`);
    process.exit(1);
}
