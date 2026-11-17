import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import { packTar, unpackTar } from './tar.mjs';

const files = [
    { name: 'manifest.json', data: Buffer.from('{"version":1}\n') },
    { name: 'chess/frame_000.avif', data: Buffer.alloc(1300, 7) },
    { name: 'chess/frame_001.avif', data: Buffer.alloc(0) },
];

test('what is packed unpacks as it was', () => {
    const back = unpackTar(packTar(files));
    assert.deepEqual(
        back.map((entry) => [entry.name, entry.data.toString('hex')]),
        files.map((entry) => [entry.name, entry.data.toString('hex')]),
    );
});

test('the same files make the same archive, byte for byte', () => {
    assert.ok(packTar(files).equals(packTar(files)));
});

test('a damaged header is refused', () => {
    const archive = packTar(files);
    archive[0] ^= 0xff;
    assert.throws(() => unpackTar(archive), /damaged header/);
});

test('an archive cut short is refused', () => {
    const archive = packTar(files);
    assert.throws(() => unpackTar(archive.subarray(0, 512 + 600)), /cut short|no end marker/);
});

test('an entry that is not a regular file is refused', () => {
    const archive = packTar([{ name: 'link', data: Buffer.alloc(0) }]);
    archive[156] = '2'.charCodeAt(0);
    // Recompute the checksum so only the type is wrong.
    archive.fill(0x20, 148, 156);
    let sum = 0;
    for (let i = 0; i < 512; i++) sum += archive[i];
    archive.write(sum.toString(8).padStart(6, '0') + '\0 ', 148, 'ascii');
    assert.throws(() => unpackTar(archive), /not a regular file/);
});

test('a name longer than a header holds is refused when packing', () => {
    assert.throws(() => packTar([{ name: 'x'.repeat(101), data: Buffer.alloc(1) }]), /longer than 100 bytes/);
});
