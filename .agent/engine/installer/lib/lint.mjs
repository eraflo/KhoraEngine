// Runs `cargo xtask lint-file` for the Claude Code PostToolUse hook and the
// pre-commit hook. Zero-dependency Node ESM.
//
// The xtask binary prebuilt in the default target (`target/debug/xtask`) runs
// directly when it is newer than every source of the xtask crate: no cargo, no
// compile, well under a second. Otherwise `cargo xtask lint-file` builds it
// first, into the default target (CARGO_TARGET_DIR is dropped), so the next run
// is direct again.

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

// Top-level folders whose Rust files lint-file reads (mirrors LINTED_ROOTS in
// xtask/src/lint/runner.rs).
export const LINTED_ROOTS = ['crates/', 'xtask/', 'examples/'];

// lint-file's exit codes: clean, findings, a file it could not read.
export const CLEAN = 0;
export const FINDINGS = 1;

// Most paths handed to one lint-file run, under the Windows command-line limit.
const CHUNK = 100;

// The path a hook names: the first non-empty argument, else `tool_input.file_path`
// of the JSON Claude Code writes on the hook's stdin.
export function hookPath(args, stdinText) {
  const arg = args.find((a) => typeof a === 'string' && a.trim() !== '' && !a.startsWith('--'));
  if (arg) return arg;
  if (!stdinText || !stdinText.trim()) return null;
  try {
    const input = JSON.parse(stdinText);
    const p = input?.tool_input?.file_path ?? input?.tool_input?.path;
    return typeof p === 'string' && p.trim() !== '' ? p : null;
  } catch {
    return null;
  }
}

// The paths of `paths` lint-file reads, repo-relative with `/`: a `.rs` file
// inside `repoRoot`, under crates/, xtask/ or examples/.
export function lintablePaths(repoRoot, paths) {
  const out = [];
  for (const p of paths) {
    if (!p) continue;
    const rel = path.relative(repoRoot, path.resolve(repoRoot, p)).split(path.sep).join('/');
    if (rel.startsWith('..') || path.isAbsolute(rel)) continue;
    if (!rel.endsWith('.rs') || !LINTED_ROOTS.some((r) => rel.startsWith(r))) continue;
    if (!out.includes(rel)) out.push(rel);
  }
  return out;
}

// The prebuilt xtask binary of the default target, when it exists.
export function prebuiltXtask(repoRoot) {
  const exe = process.platform === 'win32' ? 'xtask.exe' : 'xtask';
  const bin = path.join(repoRoot, 'target', 'debug', exe);
  return fs.existsSync(bin) ? bin : null;
}

// Whether `bin` was built after the last change to the xtask crate's sources
// (`xtask/src/**`, `xtask/Cargo.toml`).
export function isFresh(repoRoot, bin) {
  let built;
  try { built = fs.statSync(bin).mtimeMs; } catch { return false; }
  const newest = newestMtime([path.join(repoRoot, 'xtask', 'src'), path.join(repoRoot, 'xtask', 'Cargo.toml')]);
  return newest <= built;
}

function newestMtime(roots) {
  let newest = 0;
  const stack = [...roots];
  while (stack.length) {
    const p = stack.pop();
    let st;
    try { st = fs.statSync(p); } catch { continue; }
    if (st.isDirectory()) {
      for (const e of fs.readdirSync(p)) stack.push(path.join(p, e));
    } else {
      newest = Math.max(newest, st.mtimeMs);
    }
  }
  return newest;
}

// How to run lint-file: the fresh prebuilt binary, else cargo.
export function lintCommand(repoRoot) {
  const bin = prebuiltXtask(repoRoot);
  if (bin && isFresh(repoRoot, bin)) return { cmd: bin, args: ['lint-file'], cargo: false };
  return { cmd: 'cargo', args: ['xtask', 'lint-file'], cargo: true };
}

// Runs lint-file on `files` (repo-relative). Returns { status, output } where
// status is CLEAN, FINDINGS, or null when lint-file could not run; output holds
// lint-file's report (its stdout), or why it could not run.
export function runLint(repoRoot, files, { run = spawnSync } = {}) {
  if (files.length === 0) return { status: CLEAN, output: '' };
  const { cmd, args, cargo } = lintCommand(repoRoot);
  const env = { ...process.env };
  if (cargo) delete env.CARGO_TARGET_DIR; // build into the default target
  let output = '';
  let status = CLEAN;
  for (let i = 0; i < files.length; i += CHUNK) {
    const r = run(cmd, [...args, ...files.slice(i, i + CHUNK)], {
      cwd: repoRoot, env, encoding: 'utf8', windowsHide: true,
    });
    if (r.error || (r.status !== CLEAN && r.status !== FINDINGS)) {
      const why = r.error ? r.error.message : (r.stderr || r.stdout || `exit ${r.status}`).trim();
      return { status: null, output: `lint-file could not run (${cmd}): ${why}` };
    }
    output += r.stdout ?? '';
    if (r.status === FINDINGS) status = FINDINGS;
  }
  return { status, output };
}
