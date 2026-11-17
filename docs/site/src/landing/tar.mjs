// A plain tar archive (POSIX ustar, no compression), written and read here so
// the landing's frames travel as one file without depending on the `tar` a
// machine happens to have (GNU tar reads `C:\…` as a remote host; bsdtar does
// not).
//
// Only regular files are written or read. Every entry is stamped the same way
// (mode 0644, owner 0, time 0), so the same files always make the same
// archive, byte for byte: its SHA-256 names its content.

const BLOCK = 512;

/**
 * Packs files into a tar archive.
 *
 * @param {{ name: string, data: Uint8Array }[]} entries Paths use `/`, at most 100 bytes.
 * @returns {Buffer}
 */
export function packTar(entries) {
    const parts = [];
    for (const { name, data } of entries) {
        parts.push(header(name, data.length));
        parts.push(Buffer.from(data));
        const pad = (BLOCK - (data.length % BLOCK)) % BLOCK;
        if (pad) parts.push(Buffer.alloc(pad));
    }
    parts.push(Buffer.alloc(BLOCK * 2));
    return Buffer.concat(parts);
}

/**
 * Reads the regular files of a tar archive.
 *
 * Fails on a damaged header (its checksum does not match), an entry that is
 * not a regular file (a link, a folder, a device), or an archive cut short.
 *
 * @param {Uint8Array} archive
 * @returns {{ name: string, data: Buffer }[]}
 */
export function unpackTar(archive) {
    const buf = Buffer.from(archive.buffer, archive.byteOffset, archive.byteLength);
    const entries = [];
    let at = 0;
    while (at + BLOCK <= buf.length) {
        const block = buf.subarray(at, at + BLOCK);
        if (block.every((byte) => byte === 0)) return entries;
        if (checksum(block) !== octal(block, 148, 8)) {
            throw new Error(`tar: damaged header at byte ${at}`);
        }
        const type = String.fromCharCode(block[156] || 0x30);
        const prefix = text(block, 345, 155);
        const name = prefix ? `${prefix}/${text(block, 0, 100)}` : text(block, 0, 100);
        if (type !== '0') throw new Error(`tar: ${name} is not a regular file (type ${type})`);
        const size = octal(block, 124, 12);
        const start = at + BLOCK;
        if (start + size > buf.length) throw new Error(`tar: ${name} is cut short`);
        entries.push({ name, data: Buffer.from(buf.subarray(start, start + size)) });
        at = start + Math.ceil(size / BLOCK) * BLOCK;
    }
    throw new Error('tar: the archive has no end marker');
}

/** One ustar header for a regular file. */
function header(name, size) {
    const encoded = Buffer.from(name, 'utf8');
    if (encoded.length > 100) throw new Error(`tar: ${name} is longer than 100 bytes`);
    const block = Buffer.alloc(BLOCK);
    encoded.copy(block, 0);
    block.write('0000644\0', 100, 'ascii');
    block.write('0000000\0', 108, 'ascii');
    block.write('0000000\0', 116, 'ascii');
    block.write(size.toString(8).padStart(11, '0') + '\0', 124, 'ascii');
    block.write('00000000000\0', 136, 'ascii');
    block.fill(0x20, 148, 156);
    block.write('0', 156, 'ascii');
    block.write('ustar\0' + '00', 257, 'ascii');
    block.write(checksum(block).toString(8).padStart(6, '0') + '\0 ', 148, 'ascii');
    return block;
}

/** The header's checksum: the sum of its bytes, its own field read as spaces. */
function checksum(block) {
    let sum = 0;
    for (let i = 0; i < BLOCK; i++) sum += i >= 148 && i < 156 ? 0x20 : block[i];
    return sum;
}

function octal(block, offset, length) {
    const value = text(block, offset, length).trim();
    return value ? parseInt(value, 8) : 0;
}

function text(block, offset, length) {
    const field = block.subarray(offset, offset + length);
    const end = field.indexOf(0);
    return field.subarray(0, end < 0 ? length : end).toString('utf8');
}
