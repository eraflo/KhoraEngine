// Cleans cargo's target/ directory: stale incremental caches always, and the
// debug/release profiles when they grow past a size limit.
//
// rustc keeps one incremental cache per crate and per configuration, and
// nothing ever deletes the ones a configuration change leaves behind; cargo
// keeps every variant of every artifact it built. Left alone, target/ reached
// 27 GB on this repository. Run at the start of each agent session (a
// SessionStart hook), so it must be cheap and must never fail: it runs at most
// once per `minIntervalHours`, and an I/O error is reported, never thrown.

import fs from 'node:fs';
import path from 'node:path';

const DAY_MS = 86_400_000;
const HOUR_MS = 3_600_000;
const GIB = 1024 ** 3;
const STAMP = '.khora-sweep-stamp';
/** The profiles a full clean removes; `doc/` and anything else are kept. */
const PROFILES = ['debug', 'release'];

/** Paths of the incremental dirs last touched more than `maxAgeDays` ago. */
export function staleIncrementalDirs(entries, nowMs, maxAgeDays) {
  if (maxAgeDays < 0) throw new RangeError(`maxAgeDays must be >= 0, got ${maxAgeDays}`);
  const limit = maxAgeDays * DAY_MS;
  return entries.filter((e) => nowMs - e.mtimeMs > limit).map((e) => e.path);
}

/** Whether the build profiles are large enough to be dropped entirely. */
export function shouldFullClean(totalBytes, maxBytes) {
  return totalBytes > maxBytes;
}

/** Sum of the sizes of the regular files under `dir` (0 when it is missing). */
function sizeOf(dir) {
  let total = 0;
  let children;
  try {
    children = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return 0;
  }
  for (const child of children) {
    const p = path.join(dir, child.name);
    if (child.isDirectory()) total += sizeOf(p);
    else if (child.isFile()) {
      try { total += fs.statSync(p).size; } catch { /* vanished meanwhile */ }
    }
  }
  return total;
}

function subdirs(dir) {
  try {
    return fs.readdirSync(dir, { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => path.join(dir, d.name));
  } catch {
    return [];
  }
}

/** Removes `dir`; returns an error message instead of throwing. */
function remove(dir) {
  try {
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 2 });
    return null;
  } catch (e) {
    // Gone anyway (another session's sweep deleted it meanwhile): not an error.
    return fs.existsSync(dir) ? `${dir}: ${e.message}` : null;
  }
}

/** Time of the last completed sweep, or `null` when there is none worth trusting. */
function lastSweep(targetDir, nowMs) {
  try {
    const at = Date.parse(fs.readFileSync(path.join(targetDir, STAMP), 'utf8').trim());
    // A stamp from the future (clock skew) is not trusted, or it would skip forever.
    return Number.isFinite(at) && at <= nowMs ? at : null;
  } catch {
    return null;
  }
}

/** Whether `dir` lies inside one of the profiles a full clean removes. */
function inProfiles(targetDir, dir) {
  return PROFILES.some((p) => {
    const rel = path.relative(path.join(targetDir, p), dir);
    return rel !== '' && !rel.startsWith('..') && !path.isAbsolute(rel);
  });
}

/**
 * Prunes `targetDir`: incremental caches older than `maxAgeDays`, then the
 * `debug` and `release` profiles if together they still exceed `maxGb` GiB
 * (`doc` and everything else are kept, and never count toward the limit). At
 * most once per `minIntervalHours`: a younger stamp returns `skipped: true`
 * without walking the tree. With `dryRun`, reports what it would do, removes
 * nothing and ignores the stamp.
 */
export async function sweep(
  targetDir,
  { maxAgeDays = 3, maxGb = 20, dryRun = false, minIntervalHours = 12, now = Date.now() } = {},
) {
  const result = { removed: [], fullClean: false, bytesBefore: 0, bytesAfter: 0, errors: [], skipped: false };
  if (!fs.existsSync(targetDir)) return result;

  if (!dryRun && minIntervalHours > 0) {
    const last = lastSweep(targetDir, now);
    if (last !== null && now - last < minIntervalHours * HOUR_MS) return { ...result, skipped: true };
  }

  result.bytesBefore = sizeOf(targetDir);

  // (a) stale incremental caches, in every profile that has them
  const entries = [];
  for (const profile of subdirs(targetDir)) {
    for (const dir of subdirs(path.join(profile, 'incremental'))) {
      try {
        entries.push({ path: dir, mtimeMs: fs.statSync(dir).mtimeMs });
      } catch { /* vanished meanwhile */ }
    }
  }
  let staleBytes = 0;
  let staleProfileBytes = 0;
  for (const dir of staleIncrementalDirs(entries, now, maxAgeDays)) {
    if (dryRun) {
      const bytes = sizeOf(dir);
      staleBytes += bytes;
      if (inProfiles(targetDir, dir)) staleProfileBytes += bytes;
      result.removed.push(path.resolve(dir));
      continue;
    }
    const err = remove(dir);
    if (err) result.errors.push(err);
    else result.removed.push(path.resolve(dir));
  }

  // (b) the build profiles, when they are still too large
  const profileDirs = PROFILES.map((p) => path.join(targetDir, p)).filter((d) => fs.existsSync(d));
  const profileSizes = profileDirs.map(sizeOf);
  const profileBytes = profileSizes.reduce((a, b) => a + b, 0) - staleProfileBytes;
  let wouldFree = 0;
  if (shouldFullClean(profileBytes, maxGb * GIB)) {
    result.fullClean = true;
    profileDirs.forEach((dir) => {
      if (dryRun) { result.removed.push(path.resolve(dir)); return; }
      const err = remove(dir);
      if (err) result.errors.push(err);
      else result.removed.push(path.resolve(dir));
    });
    wouldFree = profileBytes;
  }

  if (dryRun) {
    result.bytesAfter = result.bytesBefore - staleBytes - wouldFree;
    return result;
  }
  result.bytesAfter = sizeOf(targetDir);
  // A full clean that could not finish (a running binary locks its file) leaves
  // the profiles over the limit: no stamp, so the next session retries.
  if (result.fullClean && result.errors.length > 0) return result;
  try {
    fs.writeFileSync(path.join(targetDir, STAMP), new Date(now).toISOString());
  } catch (e) {
    result.errors.push(`${path.join(targetDir, STAMP)}: ${e.message}`);
  }
  return result;
}
