// Tests for the target/ sweep (lib/sweep.mjs) and its CLI entry point
// (`khora-ai.mjs sweep`). Run with `node --test .agent/engine/installer/lib/`.
//
// Every filesystem test works on a real temp directory; nothing here touches
// the repository's own target/.

import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';

import { staleIncrementalDirs, shouldFullClean, sweep } from './sweep.mjs';

const DAY = 86_400_000;
const GIB = 1024 ** 3;
// A fixed "now" on a whole second, so mtimes set through utimesSync round-trip exactly.
const NOW = Math.floor(Date.UTC(2026, 8, 27, 12, 0, 0) / 1000) * 1000;

const here = path.dirname(fileURLToPath(import.meta.url));
const installerDir = path.dirname(here);

// ── fixtures ───────────────────────────────────────────────────────────────

function tmpRoot(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'khora-sweep-'));
  t.after(() => {
    try { fs.chmodSync(root, 0o755); } catch {}
    fs.rmSync(root, { recursive: true, force: true, maxRetries: 5 });
  });
  return root;
}

function writeBytes(file, bytes) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, Buffer.alloc(bytes, 0x61));
}

function setMtime(p, mtimeMs) {
  const d = new Date(mtimeMs);
  fs.utimesSync(p, d, d);
}

// target/<profile>/incremental/<name>/ holding one file of `bytes`, mtime = now - ageMs.
// The mtime is set last: writing inside a dir bumps its mtime.
function session(target, profile, name, { ageMs, bytes = 16, now = NOW }) {
  const dir = path.join(target, profile, 'incremental', name);
  writeBytes(path.join(dir, 'dep-graph.bin'), bytes);
  setMtime(dir, now - ageMs);
  return dir;
}

const norm = (paths) => paths.map((p) => path.resolve(p)).sort();

// Hold a lock that makes `sessionDir` impossible to remove, like a running
// rustc/editor process does on Windows. Returns an async release function,
// which must run before the temp dir is deleted. Writing the lock file bumps
// the session's mtime: callers re-set it afterwards.
// - Windows: a child PowerShell opens a file inside with FileShare.None.
// - POSIX: the parent incremental/ dir is made read-only (no effect as root).
async function lockDir(sessionDir) {
  if (process.platform === 'win32') {
    const f = path.join(sessionDir, 'held.lock');
    fs.writeFileSync(f, 'x');
    const ps = `$h=[System.IO.File]::Open('${f.replace(/'/g, "''")}','Open','Read','None'); ` +
      `[Console]::Out.WriteLine('locked'); [Console]::Out.Flush(); Start-Sleep -Seconds 600`;
    const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', ps],
      { stdio: ['ignore', 'pipe', 'ignore'] });
    await new Promise((resolve, reject) => {
      child.on('error', reject);
      child.stdout.on('data', (b) => { if (String(b).includes('locked')) resolve(); });
    });
    return async () => { child.kill(); if (child.exitCode === null) await once(child, 'exit'); };
  }
  const parent = path.dirname(sessionDir);
  fs.chmodSync(parent, 0o555);
  return async () => { fs.chmodSync(parent, 0o755); };
}

const canLock = !(process.platform !== 'win32' && process.getuid?.() === 0);

// ── staleIncrementalDirs ───────────────────────────────────────────────────

describe('staleIncrementalDirs', () => {
  test('selects only the entries older than maxAgeDays, in input order', () => {
    const entries = [
      { path: 'a', mtimeMs: NOW - 5 * DAY },
      { path: 'b', mtimeMs: NOW - 1 * DAY },
      { path: 'c', mtimeMs: NOW - 10 * DAY },
      { path: 'd', mtimeMs: NOW - 4 * DAY },
    ];
    assert.deepEqual(staleIncrementalDirs(entries, NOW, 3), ['a', 'c', 'd']);
  });

  test('an entry exactly maxAgeDays old is not stale; one millisecond older is', () => {
    const entries = [
      { path: 'exact', mtimeMs: NOW - 3 * DAY },
      { path: 'older', mtimeMs: NOW - 3 * DAY - 1 },
    ];
    assert.deepEqual(staleIncrementalDirs(entries, NOW, 3), ['older']);
  });

  test('maxAgeDays of 0 marks everything strictly older than now as stale', () => {
    const entries = [
      { path: 'past', mtimeMs: NOW - 1 },
      { path: 'now', mtimeMs: NOW },
      { path: 'future', mtimeMs: NOW + DAY },
    ];
    assert.deepEqual(staleIncrementalDirs(entries, NOW, 0), ['past']);
  });

  test('an empty entry list yields an empty array', () => {
    assert.deepEqual(staleIncrementalDirs([], NOW, 3), []);
  });

  test('a negative maxAgeDays throws RangeError', () => {
    assert.throws(() => staleIncrementalDirs([{ path: 'a', mtimeMs: 0 }], NOW, -1), RangeError);
  });
});

// ── shouldFullClean ────────────────────────────────────────────────────────

describe('shouldFullClean', () => {
  test('is true only when totalBytes strictly exceeds maxBytes', () => {
    assert.equal(shouldFullClean(2, 1), true);
    assert.equal(shouldFullClean(20 * GIB + 1, 20 * GIB), true);
  });

  test('is false at or under the limit', () => {
    assert.equal(shouldFullClean(1, 1), false);
    assert.equal(shouldFullClean(0, 1), false);
    assert.equal(shouldFullClean(0, 0), false);
    assert.equal(shouldFullClean(20 * GIB, 20 * GIB), false);
  });
});

// ── sweep: incremental pruning ─────────────────────────────────────────────

describe('sweep — incremental pruning', () => {
  test('removes stale debug session dirs and keeps fresh ones', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY });
    const fresh = session(target, 'debug', 's-fresh', { ageMs: 1 * DAY });

    const r = await sweep(target, { maxAgeDays: 3, now: NOW });

    assert.equal(fs.existsSync(stale), false, 'stale session must be gone');
    assert.equal(fs.existsSync(fresh), true, 'fresh session must be kept');
    assert.deepEqual(norm(r.removed), norm([stale]));
    assert.equal(r.fullClean, false);
    assert.deepEqual(r.errors, []);
  });

  test('a session exactly maxAgeDays old is kept', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const exact = session(target, 'debug', 's-exact', { ageMs: 3 * DAY });
    const older = session(target, 'debug', 's-older', { ageMs: 3 * DAY + 1000 });

    const r = await sweep(target, { maxAgeDays: 3, now: NOW });

    assert.equal(fs.existsSync(exact), true);
    assert.equal(fs.existsSync(older), false);
    assert.deepEqual(norm(r.removed), norm([older]));
  });

  test('maxAgeDays defaults to 3', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const young = session(target, 'debug', 's-young', { ageMs: 2.9 * DAY });
    const old = session(target, 'debug', 's-old', { ageMs: 3.1 * DAY });

    const r = await sweep(target, { now: NOW });

    assert.equal(fs.existsSync(young), true);
    assert.equal(fs.existsSync(old), false);
    assert.deepEqual(norm(r.removed), norm([old]));
  });

  test('prunes the release profile and any other profile holding incremental/', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const dbg = session(target, 'debug', 's-1', { ageMs: 10 * DAY });
    const rel = session(target, 'release', 's-2', { ageMs: 10 * DAY });
    const custom = session(target, 'profiling', 's-3', { ageMs: 10 * DAY });
    const relFresh = session(target, 'release', 's-4', { ageMs: 1 * DAY });

    const r = await sweep(target, { maxAgeDays: 3, now: NOW });

    for (const d of [dbg, rel, custom]) assert.equal(fs.existsSync(d), false, `${d} must be gone`);
    assert.equal(fs.existsSync(relFresh), true);
    assert.deepEqual(norm(r.removed), norm([dbg, rel, custom]));
  });

  test('leaves everything outside incremental/ untouched, however old', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const deps = path.join(target, 'debug', 'deps', 'libold.rlib');
    const build = path.join(target, 'debug', 'build', 'old-123');
    const doc = path.join(target, 'doc', 'index.html');
    writeBytes(deps, 32);
    writeBytes(path.join(build, 'output'), 32);
    writeBytes(doc, 32);
    for (const p of [deps, build, doc]) setMtime(p, NOW - 100 * DAY);
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY });

    const r = await sweep(target, { maxAgeDays: 3, now: NOW });

    for (const p of [deps, build, doc]) assert.equal(fs.existsSync(p), true, `${p} must be kept`);
    assert.equal(fs.existsSync(path.join(target, 'debug', 'incremental')), true, 'incremental/ itself stays');
    assert.deepEqual(norm(r.removed), norm([stale]));
  });

  test('bytesBefore and bytesAfter count the file bytes before and after the sweep', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    session(target, 'debug', 's-stale', { ageMs: 10 * DAY, bytes: 300 });
    session(target, 'debug', 's-fresh', { ageMs: 1 * DAY, bytes: 200 });
    writeBytes(path.join(target, 'doc', 'index.html'), 50);

    const r = await sweep(target, { maxAgeDays: 3, now: NOW });

    assert.equal(r.bytesBefore, 550);
    assert.equal(r.bytesAfter, 250);
  });
});

// ── sweep: size threshold / full clean ─────────────────────────────────────

describe('sweep — full clean over maxGb', () => {
  test('over maxGb removes debug/ and release/ but keeps doc/', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    writeBytes(path.join(target, 'debug', 'deps', 'big.rlib'), 2000);
    writeBytes(path.join(target, 'release', 'deps', 'big.rlib'), 2000);
    writeBytes(path.join(target, 'doc', 'index.html'), 100);

    const r = await sweep(target, { maxGb: 1e-6, now: NOW }); // 1e-6 GiB ≈ 1073 bytes

    assert.equal(r.fullClean, true);
    assert.equal(fs.existsSync(path.join(target, 'debug')), false, 'debug/ must be gone');
    assert.equal(fs.existsSync(path.join(target, 'release')), false, 'release/ must be gone');
    assert.equal(fs.existsSync(path.join(target, 'doc', 'index.html')), true, 'doc/ must be kept');
    assert.equal(r.bytesBefore, 4100);
    assert.equal(r.bytesAfter, 100);
    assert.deepEqual(r.errors, []);
  });

  test('full clean works when only debug/ exists', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    writeBytes(path.join(target, 'debug', 'deps', 'big.rlib'), 2000);

    const r = await sweep(target, { maxGb: 1e-6, now: NOW });

    assert.equal(r.fullClean, true);
    assert.equal(fs.existsSync(path.join(target, 'debug')), false);
    assert.deepEqual(r.errors, []);
  });

  test('at or under maxGb nothing outside stale incremental/ is removed', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const lib = path.join(target, 'debug', 'deps', 'small.rlib');
    writeBytes(lib, 500);

    const r = await sweep(target, { maxGb: 1e-6, now: NOW });

    assert.equal(r.fullClean, false);
    assert.equal(fs.existsSync(lib), true);
    assert.deepEqual(r.removed, []);
  });

  test('maxGb defaults to 20 GiB (a small tree is never fully cleaned)', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const lib = path.join(target, 'debug', 'deps', 'x.rlib');
    writeBytes(lib, 4096);

    const r = await sweep(target, { now: NOW });

    assert.equal(r.fullClean, false);
    assert.equal(fs.existsSync(lib), true);
  });

  test('size is measured after pruning: pruning below the limit prevents a full clean', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, bytes: 2000 });
    const fresh = session(target, 'debug', 's-fresh', { ageMs: 1 * DAY, bytes: 100 });

    const r = await sweep(target, { maxAgeDays: 3, maxGb: 1e-6, now: NOW });

    assert.equal(r.fullClean, false, 'after pruning only 100 bytes remain, under ~1073');
    assert.equal(fs.existsSync(stale), false);
    assert.equal(fs.existsSync(fresh), true);
    assert.equal(r.bytesBefore, 2100);
    assert.equal(r.bytesAfter, 100);
  });
});

// ── sweep: dry run ─────────────────────────────────────────────────────────

describe('sweep — dryRun', () => {
  test('reports the stale sessions but removes nothing', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY });
    const staleRel = session(target, 'release', 's-stale', { ageMs: 10 * DAY });
    const fresh = session(target, 'debug', 's-fresh', { ageMs: 1 * DAY });

    const r = await sweep(target, { maxAgeDays: 3, dryRun: true, now: NOW });

    assert.deepEqual(norm(r.removed), norm([stale, staleRel]));
    for (const d of [stale, staleRel, fresh]) assert.equal(fs.existsSync(d), true, `${d} must survive a dry run`);
    assert.deepEqual(r.errors, []);
  });

  test('reports a full clean but keeps debug/ and release/', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const dbg = path.join(target, 'debug', 'deps', 'big.rlib');
    const rel = path.join(target, 'release', 'deps', 'big.rlib');
    writeBytes(dbg, 2000);
    writeBytes(rel, 2000);

    const r = await sweep(target, { maxGb: 1e-6, dryRun: true, now: NOW });

    assert.equal(r.fullClean, true);
    assert.equal(fs.existsSync(dbg), true);
    assert.equal(fs.existsSync(rel), true);
    assert.equal(r.bytesBefore, 4000);
  });
});

// ── sweep: missing target / errors ─────────────────────────────────────────

describe('sweep — robustness', () => {
  test('a missing targetDir is a no-op returning zeros and no error', async (t) => {
    const target = path.join(tmpRoot(t), 'does-not-exist');

    const r = await sweep(target, { now: NOW });

    assert.deepEqual(r, { removed: [], fullClean: false, bytesBefore: 0, bytesAfter: 0, errors: [], skipped: false });
    assert.equal(fs.existsSync(target), false, 'the sweep must not create target/');
  });

  test('a target/ with no incremental/ at all is a no-op', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    fs.mkdirSync(target, { recursive: true });

    const r = await sweep(target, { now: NOW });

    assert.deepEqual(r.removed, []);
    assert.equal(r.fullClean, false);
    assert.deepEqual(r.errors, []);
  });

  test('a session that cannot be removed is recorded in errors and the sweep continues', { skip: !canLock && 'cannot simulate a lock as root' }, async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const locked = session(target, 'debug', 's-locked', { ageMs: 10 * DAY });
    const other = session(target, 'release', 's-other', { ageMs: 10 * DAY });
    const release = await lockDir(locked);
    setMtime(locked, NOW - 10 * DAY);
    let r;
    try {
      await assert.doesNotReject(async () => { r = await sweep(target, { maxAgeDays: 3, now: NOW }); });
    } finally { await release(); }

    assert.ok(r.errors.length >= 1, 'the failed removal must be reported');
    assert.ok(r.errors.some((e) => e.includes('s-locked')), `an error must name the locked dir: ${r.errors}`);
    assert.equal(fs.existsSync(other), false, 'the sweep must continue past the failure');
    assert.ok(norm(r.removed).includes(path.resolve(other)));
    assert.ok(!norm(r.removed).includes(path.resolve(locked)), 'a dir that was not removed is not reported as removed');
  });

  test('a locked file during a full clean is recorded in errors, not thrown', { skip: !canLock && 'cannot simulate a lock as root' }, async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const locked = session(target, 'debug', 's-locked', { ageMs: 0, bytes: 2000 });
    writeBytes(path.join(target, 'release', 'deps', 'big.rlib'), 2000);
    const release = await lockDir(locked);
    let r;
    try {
      await assert.doesNotReject(async () => { r = await sweep(target, { maxGb: 1e-6, now: NOW }); });
    } finally { await release(); }

    assert.equal(r.fullClean, true);
    assert.ok(r.errors.length >= 1, 'the failed removal must be reported');
    assert.equal(fs.existsSync(path.join(target, 'release')), false, 'release/ is still cleaned');
  });
});

// ── CLI: khora-ai.mjs sweep ────────────────────────────────────────────────
// The CLI sweeps <repo root>/target, and the repo root is derived from the
// installer's own location. So each test copies the installer into a fake
// repo (a temp dir with a Cargo.toml) and runs the copy.

function fakeRepo(t) {
  const root = tmpRoot(t);
  fs.writeFileSync(path.join(root, 'Cargo.toml'), '[workspace]\n');
  const dest = path.join(root, '.agent', 'engine', 'installer');
  fs.cpSync(installerDir, dest, { recursive: true, filter: (src) => !src.endsWith('.test.mjs') });
  return { root, target: path.join(root, 'target'), bin: path.join(dest, 'bin', 'khora-ai.mjs') };
}

function runCli(repo, args) {
  return spawnSync(process.execPath, [repo.bin, 'sweep', ...args], { cwd: repo.root, encoding: 'utf8' });
}

function oneLine(stdout) {
  return stdout.split(/\r?\n/).filter((l) => l.trim() !== '');
}

describe('CLI — khora-ai.mjs sweep', () => {
  test('--days N prunes the <repo>/target incremental sessions older than N days, exit 0, one-line summary', (t) => {
    const repo = fakeRepo(t);
    const now = Date.now();
    const stale = session(repo.target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    const fresh = session(repo.target, 'debug', 's-fresh', { ageMs: 60_000, now });

    const res = runCli(repo, ['--days', '3']);

    assert.equal(res.status, 0, res.stderr);
    assert.equal(fs.existsSync(stale), false);
    assert.equal(fs.existsSync(fresh), true);
    assert.equal(oneLine(res.stdout).length, 1, `expected a one-line summary, got:\n${res.stdout}`);
  });

  test('--days is honoured (a 2-day-old session survives the default but not --days 1)', (t) => {
    const repo = fakeRepo(t);
    const now = Date.now();
    const twoDays = session(repo.target, 'debug', 's-2d', { ageMs: 2 * DAY, now });

    const first = runCli(repo, []);
    assert.equal(first.status, 0, first.stderr);
    assert.equal(fs.existsSync(twoDays), true, 'default of 3 days keeps a 2-day-old session');

    const second = runCli(repo, ['--days', '1', '--force']);
    assert.equal(second.status, 0, second.stderr);
    assert.equal(fs.existsSync(twoDays), false, '--days 1 removes it');
  });

  test('--dry-run removes nothing', (t) => {
    const repo = fakeRepo(t);
    const now = Date.now();
    const stale = session(repo.target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeBytes(path.join(repo.target, 'release', 'deps', 'big.rlib'), 2000);

    const res = runCli(repo, ['--dry-run', '--max-gb', '0.000001']);

    assert.equal(res.status, 0, res.stderr);
    assert.equal(fs.existsSync(stale), true);
    assert.equal(fs.existsSync(path.join(repo.target, 'release', 'deps', 'big.rlib')), true);
  });

  test('--max-gb G triggers the full clean of debug/ and release/, keeping doc/', (t) => {
    const repo = fakeRepo(t);
    writeBytes(path.join(repo.target, 'debug', 'deps', 'big.rlib'), 2000);
    writeBytes(path.join(repo.target, 'release', 'deps', 'big.rlib'), 2000);
    writeBytes(path.join(repo.target, 'doc', 'index.html'), 10);

    const res = runCli(repo, ['--max-gb', '0.000001']);

    assert.equal(res.status, 0, res.stderr);
    assert.equal(fs.existsSync(path.join(repo.target, 'debug')), false);
    assert.equal(fs.existsSync(path.join(repo.target, 'release')), false);
    assert.equal(fs.existsSync(path.join(repo.target, 'doc', 'index.html')), true);
  });

  test('a repo without target/ exits 0', (t) => {
    const repo = fakeRepo(t);

    const res = runCli(repo, []);

    assert.equal(res.status, 0, res.stderr);
    assert.equal(fs.existsSync(repo.target), false);
  });

  test('exit code is 0 even when a removal fails', { skip: !canLock && 'cannot simulate a lock as root' }, async (t) => {
    const repo = fakeRepo(t);
    const now = Date.now();
    const locked = session(repo.target, 'debug', 's-locked', { ageMs: 10 * DAY, now });
    const release = await lockDir(locked);
    setMtime(locked, now - 10 * DAY);
    let res;
    try { res = runCli(repo, ['--days', '3']); } finally { await release(); }

    assert.equal(res.status, 0, `a cleanup must never fail a session start:\n${res.stderr}`);
    assert.equal(fs.existsSync(locked), true);
  });
});

// ── dry runs and CLI values ─────────────────────────────────────────────────

describe('dry run agrees with the real run', () => {
  test('dryRun reports the bytesAfter the real sweep reaches when a full clean would run', async (t) => {
    const build = (root) => {
      const target = path.join(root, 'target');
      writeBytes(path.join(target, 'debug', 'deps', 'big.rlib'), 2000);
      writeBytes(path.join(target, 'release', 'deps', 'big.rlib'), 2000);
      writeBytes(path.join(target, 'doc', 'index.html'), 100);
      return target;
    };

    const dry = await sweep(build(tmpRoot(t)), { maxGb: 1e-6, dryRun: true, now: NOW });
    const real = await sweep(build(tmpRoot(t)), { maxGb: 1e-6, now: NOW });

    assert.equal(real.bytesAfter, 100);
    assert.equal(dry.bytesAfter, real.bytesAfter, 'a dry run must predict the size the real run leaves');
  });
});

describe('CLI values that are not numbers fall back, never destroy', () => {
  test('an empty --max-gb value falls back to 20 GiB instead of 0 (no full clean)', (t) => {
    const repo = fakeRepo(t);
    const lib = path.join(repo.target, 'debug', 'deps', 'x.rlib');
    writeBytes(lib, 4096);

    const res = runCli(repo, ['--max-gb', '']);

    assert.equal(res.status, 0, res.stderr);
    assert.equal(fs.existsSync(lib), true, `--max-gb "" was read as 0 and wiped debug/: ${res.stdout}`);
  });

  test('an empty --days value falls back to 3 days instead of 0', (t) => {
    const repo = fakeRepo(t);
    const twoDays = session(repo.target, 'debug', 's-2d', { ageMs: 2 * DAY, now: Date.now() });

    const res = runCli(repo, ['--days', '']);

    assert.equal(res.status, 0, res.stderr);
    assert.equal(fs.existsSync(twoDays), true, `--days "" was read as 0 and pruned a 2-day-old cache: ${res.stdout}`);
  });

  test('an unknown flag (a mistyped --dry-run) removes nothing', (t) => {
    const repo = fakeRepo(t);
    const lib = path.join(repo.target, 'debug', 'deps', 'big.rlib');
    writeBytes(lib, 2000);

    const res = runCli(repo, ['--dryrun', '--max-gb', '0.000001']);

    assert.equal(res.status, 0, res.stderr);
    assert.equal(fs.existsSync(lib), true, `"--dryrun" was ignored and a real full clean ran: ${res.stdout}`);
  });
});

// ── throttle (minIntervalHours + targetDir/.khora-sweep-stamp) ──
// Throttle tests use the real clock (rounded to a whole second) as `now`, so
// they hold whether the implementation compares the stamp to the `now` option
// or to Date.now().

const HOUR = 3_600_000;
const STAMP = '.khora-sweep-stamp';
const realNow = () => Math.floor(Date.now() / 1000) * 1000;
const stampPath = (target) => path.join(target, STAMP);
const writeStamp = (target, ms) => {
  fs.mkdirSync(target, { recursive: true });
  fs.writeFileSync(stampPath(target), new Date(ms).toISOString());
};
const readStampMs = (target) => Date.parse(fs.readFileSync(stampPath(target), 'utf8').trim());
const SKIPPED = { removed: [], fullClean: false, bytesBefore: 0, bytesAfter: 0, errors: [], skipped: true };

describe('sweep — throttle', () => {
  test('a result that ran carries skipped: false', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    session(target, 'debug', 's-stale', { ageMs: 10 * DAY });

    const r = await sweep(target, { now: NOW });

    assert.equal(r.skipped, false);
  });

  test('a missing targetDir carries skipped: false and is not created for the stamp', async (t) => {
    const target = path.join(tmpRoot(t), 'does-not-exist');

    const r = await sweep(target, { now: NOW });

    assert.equal(r.skipped, false);
    assert.equal(fs.existsSync(target), false, 'writing the stamp must not create target/');
  });

  test('a stamp younger than minIntervalHours skips: exact skipped result, nothing removed', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeBytes(path.join(target, 'release', 'deps', 'big.rlib'), 2000);
    writeStamp(target, now - 1 * HOUR);

    const r = await sweep(target, { minIntervalHours: 12, maxGb: 1e-6, now });

    assert.deepEqual(r, SKIPPED);
    assert.equal(fs.existsSync(stale), true, 'a skipped sweep removes nothing');
    assert.equal(fs.existsSync(path.join(target, 'release')), true, 'a skipped sweep never full-cleans');
    assert.equal(readStampMs(target), now - 1 * HOUR, 'a skipped sweep does not refresh the stamp');
  });

  test('minIntervalHours defaults to 12: an 11-hour-old stamp skips, a 13-hour-old one runs', async (t) => {
    const now = realNow();
    const young = path.join(tmpRoot(t), 'target');
    const youngStale = session(young, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeStamp(young, now - 11 * HOUR);
    const old = path.join(tmpRoot(t), 'target');
    const oldStale = session(old, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeStamp(old, now - 13 * HOUR);

    const ry = await sweep(young, { now });
    const ro = await sweep(old, { now });

    assert.equal(ry.skipped, true);
    assert.equal(fs.existsSync(youngStale), true);
    assert.equal(ro.skipped, false);
    assert.equal(fs.existsSync(oldStale), false);
  });

  test('a stamp exactly minIntervalHours old runs; one minute younger skips', async (t) => {
    const now = realNow();
    const exact = path.join(tmpRoot(t), 'target');
    writeStamp(exact, now - 12 * HOUR);
    const younger = path.join(tmpRoot(t), 'target');
    writeStamp(younger, now - 12 * HOUR + 60_000);

    assert.equal((await sweep(exact, { minIntervalHours: 12, now })).skipped, false);
    assert.equal((await sweep(younger, { minIntervalHours: 12, now })).skipped, true);
  });

  test('minIntervalHours honours a custom value (2 h: a 3-hour-old stamp runs)', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeStamp(target, now - 3 * HOUR);

    const r = await sweep(target, { minIntervalHours: 2, now });

    assert.equal(r.skipped, false);
    assert.equal(fs.existsSync(stale), false);
  });

  test('minIntervalHours: 0 always runs, even with a stamp written this instant', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeStamp(target, now);

    const r = await sweep(target, { minIntervalHours: 0, now });

    assert.equal(r.skipped, false);
    assert.equal(fs.existsSync(stale), false);
  });

  test('a completed non-dry run writes the stamp as an ISO time close to now', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    session(target, 'debug', 's-stale', { ageMs: 10 * DAY, now });

    await sweep(target, { now });

    assert.equal(fs.existsSync(stampPath(target)), true, 'the stamp must be written');
    const raw = fs.readFileSync(stampPath(target), 'utf8').trim();
    const ms = Date.parse(raw);
    assert.ok(Number.isFinite(ms), `stamp is not a parsable time: ${JSON.stringify(raw)}`);
    assert.equal(new Date(ms).toISOString(), raw, 'the stamp content is an ISO-8601 time');
    assert.ok(Math.abs(ms - now) < 60_000, `stamp ${raw} is not close to now ${new Date(now).toISOString()}`);
  });

  test('a run writes the stamp even when target/ holds nothing to remove', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    fs.mkdirSync(target, { recursive: true });

    await sweep(target, { now: realNow() });

    assert.equal(fs.existsSync(stampPath(target)), true);
  });

  test('a run refreshes an expired stamp, and the next sweep inside the interval is skipped', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    writeStamp(target, now - 100 * HOUR);

    const first = await sweep(target, { now });
    const late = session(target, 'debug', 's-late', { ageMs: 10 * DAY, now });
    const second = await sweep(target, { now });

    assert.equal(first.skipped, false);
    assert.ok(Math.abs(readStampMs(target) - now) < 60_000, 'the old stamp must be refreshed');
    assert.deepEqual(second, SKIPPED);
    assert.equal(fs.existsSync(late), true);
  });

  test('the stamp file does not count in bytesAfter of the run that writes it', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    session(target, 'debug', 's-fresh', { ageMs: 1 * DAY, bytes: 200, now: realNow() });

    const r = await sweep(target, { now: realNow() });

    assert.equal(r.bytesBefore, 200);
    assert.equal(r.bytesAfter, 200);
    assert.equal(r.skipped, false);
  });

  test('a dry run ignores a young stamp and runs', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeStamp(target, now - 1 * HOUR);

    const r = await sweep(target, { dryRun: true, now });

    assert.equal(r.skipped, false);
    assert.deepEqual(norm(r.removed), norm([stale]));
    assert.equal(fs.existsSync(stale), true);
  });

  test('a dry run never writes the stamp, nor refreshes an existing one', async (t) => {
    const now = realNow();
    const fresh = path.join(tmpRoot(t), 'target');
    session(fresh, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    const old = path.join(tmpRoot(t), 'target');
    writeStamp(old, now - 100 * HOUR);

    const rf = await sweep(fresh, { dryRun: true, now });
    const ro = await sweep(old, { dryRun: true, now });

    assert.equal(rf.skipped, false);
    assert.equal(ro.skipped, false);
    assert.equal(fs.existsSync(stampPath(fresh)), false, 'a dry run must not create the stamp');
    assert.equal(readStampMs(old), now - 100 * HOUR, 'a dry run must not refresh the stamp');
  });

  test('an unparsable stamp means "run", and is replaced by a valid one', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    fs.writeFileSync(stampPath(target), 'not a date');

    const r = await sweep(target, { now });

    assert.equal(r.skipped, false);
    assert.equal(fs.existsSync(stale), false);
    assert.ok(Math.abs(readStampMs(target) - now) < 60_000, 'the garbage stamp must be overwritten');
  });

  test('an empty stamp file means "run"', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    fs.writeFileSync(stampPath(target), '');

    const r = await sweep(target, { now });

    assert.equal(r.skipped, false);
    assert.equal(fs.existsSync(stale), false);
  });

  test('a stamp dated in the future (clock skew) is invalid: the sweep runs and rewrites a valid stamp', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeStamp(target, now + 1 * HOUR);

    const r = await sweep(target, { now });

    assert.equal(r.skipped, false, 'a future stamp must never throttle, or a skewed clock skips forever');
    assert.equal(fs.existsSync(stale), false);
    const ms = readStampMs(target);
    assert.ok(Math.abs(ms - now) < 60_000, `the future stamp must be rewritten close to now, got ${new Date(ms).toISOString()}`);
  });

  test('an unreadable stamp (a directory in its place) means "run" and never throws', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    fs.mkdirSync(stampPath(target));
    let r;

    await assert.doesNotReject(async () => { r = await sweep(target, { now }); });

    assert.equal(r.skipped, false);
    assert.equal(fs.existsSync(stale), false);
  });

  test('the stamp is written even when a removal failed (errors non-empty)', { skip: !canLock && 'cannot simulate a lock as root' }, async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    const locked = session(target, 'debug', 's-locked', { ageMs: 10 * DAY, now });
    const release = await lockDir(locked);
    setMtime(locked, now - 10 * DAY);
    let r;
    try { r = await sweep(target, { maxAgeDays: 3, now }); } finally { await release(); }

    assert.ok(r.errors.length >= 1, 'precondition: the locked dir must fail to be removed');
    assert.equal(r.skipped, false);
    assert.equal(fs.existsSync(stampPath(target)), true, 'the stamp must be written despite the errors');
  });
});

// ── the full-clean threshold measures debug/ + release/ only ──

describe('sweep — full-clean threshold on the profiles', () => {
  test('a large doc/ never triggers a full clean of small profiles', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const lib = path.join(target, 'debug', 'deps', 'small.rlib');
    writeBytes(lib, 100);
    writeBytes(path.join(target, 'doc', 'big.html'), 2000);

    const r = await sweep(target, { maxGb: 1e-6, now: NOW }); // ≈ 1073 bytes

    assert.equal(r.fullClean, false, 'debug/ + release/ hold 100 bytes, under the limit');
    assert.equal(fs.existsSync(lib), true);
    assert.deepEqual(r.removed, []);
  });

  test('any other large folder (a custom profile, package/) never triggers a full clean', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const lib = path.join(target, 'release', 'deps', 'small.rlib');
    writeBytes(lib, 100);
    writeBytes(path.join(target, 'profiling', 'deps', 'big.rlib'), 2000);
    writeBytes(path.join(target, 'package', 'crate.tar'), 2000);

    const r = await sweep(target, { maxGb: 1e-6, now: NOW });

    assert.equal(r.fullClean, false);
    assert.equal(fs.existsSync(lib), true);
  });

  test('the profile size is measured after pruning, doc/ excluded', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const stale = session(target, 'debug', 's-stale', { ageMs: 10 * DAY, bytes: 2000 });
    const fresh = session(target, 'debug', 's-fresh', { ageMs: 1 * DAY, bytes: 100 });
    writeBytes(path.join(target, 'doc', 'big.html'), 2000);

    const r = await sweep(target, { maxAgeDays: 3, maxGb: 1e-6, now: NOW });

    assert.equal(r.fullClean, false, '100 bytes left in debug/ after pruning; doc/ does not count');
    assert.equal(fs.existsSync(stale), false);
    assert.equal(fs.existsSync(fresh), true);
  });

  test('bytesBefore/bytesAfter still measure the whole target/ when doc/ is large', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    writeBytes(path.join(target, 'debug', 'deps', 'small.rlib'), 100);
    writeBytes(path.join(target, 'doc', 'big.html'), 2000);

    const r = await sweep(target, { maxGb: 1e-6, now: NOW });

    assert.equal(r.bytesBefore, 2100);
    assert.equal(r.bytesAfter, 2100, 'nothing is removed, the whole target/ is still counted');
  });

  test('debug/ + release/ summed past the limit still full-clean, keeping a large doc/ (guard)', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    writeBytes(path.join(target, 'debug', 'deps', 'a.rlib'), 600);
    writeBytes(path.join(target, 'release', 'deps', 'b.rlib'), 600);
    writeBytes(path.join(target, 'doc', 'big.html'), 5000);

    const r = await sweep(target, { maxGb: 1e-6, now: NOW });

    assert.equal(r.fullClean, true, 'neither profile alone exceeds ~1073 bytes, their sum does');
    assert.equal(fs.existsSync(path.join(target, 'debug')), false);
    assert.equal(fs.existsSync(path.join(target, 'release')), false);
    assert.equal(fs.existsSync(path.join(target, 'doc', 'big.html')), true);
    assert.equal(r.bytesBefore, 6200);
    assert.equal(r.bytesAfter, 5000);
  });

  test('a dry run applies the same profile-only threshold', async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    writeBytes(path.join(target, 'debug', 'deps', 'small.rlib'), 100);
    writeBytes(path.join(target, 'doc', 'big.html'), 2000);

    const r = await sweep(target, { maxGb: 1e-6, dryRun: true, now: NOW });

    assert.equal(r.fullClean, false);
    assert.deepEqual(r.removed, []);
    assert.equal(r.bytesAfter, 2100);
  });
});

// ── CLI --force and the "skipped" summary ─────────────────────

describe('CLI — throttle', () => {
  test('a young stamp prints a one-line "skipped" summary, exit 0, removes nothing', (t) => {
    const repo = fakeRepo(t);
    const now = realNow();
    const stale = session(repo.target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeStamp(repo.target, now - 1 * HOUR);

    const res = runCli(repo, []);

    assert.equal(res.status, 0, res.stderr);
    assert.equal(fs.existsSync(stale), true, `the sweep must be throttled: ${res.stdout}`);
    const lines = oneLine(res.stdout);
    assert.equal(lines.length, 1, `expected a one-line summary, got:\n${res.stdout}`);
    assert.match(lines[0], /skipped/i);
  });

  test('--force runs despite a young stamp (and the same call without it is skipped)', (t) => {
    const repo = fakeRepo(t);
    const now = realNow();
    const stale = session(repo.target, 'debug', 's-stale', { ageMs: 10 * DAY, now });
    writeStamp(repo.target, now - 1 * HOUR);

    const throttled = runCli(repo, ['--days', '3']);
    assert.equal(throttled.status, 0, throttled.stderr);
    assert.equal(fs.existsSync(stale), true, `without --force the sweep must be skipped: ${throttled.stdout}`);

    const forced = runCli(repo, ['--days', '3', '--force']);
    assert.equal(forced.status, 0, forced.stderr);
    assert.equal(fs.existsSync(stale), false, `--force must run the sweep: ${forced.stdout}`);
    const lines = oneLine(forced.stdout);
    assert.equal(lines.length, 1, `expected a one-line summary, got:\n${forced.stdout}`);
    assert.doesNotMatch(lines[0], /skipped/i);
  });

  test('a CLI run writes <repo>/target/.khora-sweep-stamp and the next call is skipped', (t) => {
    const repo = fakeRepo(t);
    const now = realNow();
    fs.mkdirSync(repo.target, { recursive: true });

    const first = runCli(repo, []);
    const late = session(repo.target, 'debug', 's-late', { ageMs: 10 * DAY, now });
    const second = runCli(repo, []);

    assert.equal(first.status, 0, first.stderr);
    assert.equal(fs.existsSync(stampPath(repo.target)), true, 'the CLI run must write the stamp');
    assert.equal(second.status, 0, second.stderr);
    assert.match(oneLine(second.stdout)[0] ?? '', /skipped/i);
    assert.equal(fs.existsSync(late), true);
  });
});

// ── a failed full clean must not throttle its retry ────
// On Windows, fs.rmSync stops at the first file it cannot delete (a running
// target/debug/*.exe, a proc-macro .dll loaded by rust-analyzer), so a full
// clean can free nothing at all. The stamp it still writes then postpones the
// retry by minIntervalHours, although debug/ + release/ are still over maxGb.

describe('a failed full clean is retried at the next session', () => {
  test('profiles still over maxGb after a failed full clean: the next sweep inside the interval runs', { skip: !canLock && 'cannot simulate a lock as root' }, async (t) => {
    const target = path.join(tmpRoot(t), 'target');
    const now = realNow();
    // cargo's own first entry: rmSync walks debug/ in name order and stops at the lock.
    const locked = path.join(target, 'debug', '.fingerprint');
    fs.mkdirSync(locked, { recursive: true });
    writeBytes(path.join(target, 'debug', 'deps', 'big.rlib'), 5000);
    const release = await lockDir(locked);
    let first;
    try { first = await sweep(target, { maxGb: 1e-6, now }); } finally { await release(); }

    assert.equal(first.fullClean, true, 'precondition: a full clean was attempted');
    assert.ok(first.errors.length >= 1, 'precondition: the full clean failed');
    assert.ok(first.bytesAfter > 1e-6 * GIB, `precondition: the profiles are still over the limit (${first.bytesAfter} bytes)`);

    // The lock is gone (the editor was closed); a session opens an hour later.
    const second = await sweep(target, { maxGb: 1e-6, now: now + 1 * HOUR });

    assert.equal(second.skipped, false,
      `the stamp of a failed full clean throttled the retry; debug/ still holds ${first.bytesAfter} bytes`);
    assert.equal(fs.existsSync(path.join(target, 'debug')), false);
  });
});
