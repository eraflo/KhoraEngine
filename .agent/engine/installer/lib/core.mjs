// Core helpers for the khora-ai installer. Zero-dependency, Node ESM.
// Shared verbatim between the engine and gamedev installers; the profile is
// derived from the installer's own path, so the same code serves both.

import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

// ── Context ───────────────────────────────────────────────────────────────
// Derive every path from this file's location:
//   .agent/<profile>/installer/lib/core.mjs
//   profileDir = up 2, agentDir = up 3, repoRoot = up 4.
export function makeContext() {
  const here = path.dirname(fileURLToPath(import.meta.url)); // .../installer/lib
  const installerDir = path.dirname(here);                   // .../installer
  const profileDir = path.dirname(installerDir);             // .agent/<profile>
  const agentDir = path.dirname(profileDir);                 // .agent
  let repoRoot = path.dirname(agentDir);                     // repo root (default)
  // Prefer the nearest ancestor that actually looks like a repo root.
  for (let d = repoRoot; ; d = path.dirname(d)) {
    if (exists(path.join(d, '.git')) || exists(path.join(d, 'Cargo.toml'))) { repoRoot = d; break; }
    if (path.dirname(d) === d) break;
  }
  const profile = path.basename(profileDir); // "engine" | "gamedev"
  return { repoRoot, agentDir, profileDir, installerDir, profile };
}

// ── Logging ───────────────────────────────────────────────────────────────
export const log = {
  info: (m) => console.log(`  ${m}`),
  step: (m) => console.log(`▸ ${m}`),
  ok: (m) => console.log(`  ✓ ${m}`),
  warn: (m) => console.warn(`  ! ${m}`),
  err: (m) => console.error(`  ✗ ${m}`),
};

// ── fs helpers ────────────────────────────────────────────────────────────
export function exists(p) { try { fs.accessSync(p); return true; } catch { return false; } }

export function writeFileRecording(ctx, absPath, content, generated) {
  fs.mkdirSync(path.dirname(absPath), { recursive: true });
  fs.writeFileSync(absPath, content, 'utf8');
  record(ctx, generated, absPath);
}

export function copyTreeRecording(ctx, srcDir, destDir, generated, transform) {
  if (!exists(srcDir)) return;
  for (const entry of fs.readdirSync(srcDir, { withFileTypes: true })) {
    const src = path.join(srcDir, entry.name);
    const dest = path.join(destDir, entry.name);
    if (entry.isDirectory()) {
      copyTreeRecording(ctx, src, dest, generated, transform);
    } else {
      let content = fs.readFileSync(src, 'utf8');
      let outName = entry.name;
      if (transform) { const r = transform(content, src, entry.name); content = r.content; outName = r.name ?? outName; }
      writeFileRecording(ctx, path.join(destDir, outName), content, generated);
    }
  }
}

// repo-root-relative, posix-style path (for .gitignore + manifest).
export function rel(ctx, absPath) {
  return path.relative(ctx.repoRoot, absPath).split(path.sep).join('/');
}

function record(ctx, generated, absPath) {
  const r = rel(ctx, absPath);
  if (!generated.includes(r)) generated.push(r);
}

// ── Managed block helpers ─────────────────────────────────────────────────
// Replace (or insert) a `BEGIN..END`-delimited block inside a text file.
export function upsertManagedBlock(filePath, beginMark, endMark, body, { create = true } = {}) {
  let text = exists(filePath) ? fs.readFileSync(filePath, 'utf8') : (create ? '' : null);
  if (text === null) return false;
  const block = `${beginMark}\n${body}\n${endMark}`;
  const re = new RegExp(`${escapeRe(beginMark)}[\\s\\S]*?${escapeRe(endMark)}`);
  if (re.test(text)) {
    text = text.replace(re, block);
  } else {
    text = text.trimEnd() + (text.trim() ? '\n\n' : '') + block + '\n';
  }
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, text, 'utf8');
  return true;
}

export function removeManagedBlock(filePath, beginMark, endMark) {
  if (!exists(filePath)) return;
  let text = fs.readFileSync(filePath, 'utf8');
  const re = new RegExp(`\\n*${escapeRe(beginMark)}[\\s\\S]*?${escapeRe(endMark)}\\n*`);
  text = text.replace(re, '\n');
  fs.writeFileSync(filePath, text.replace(/\n{3,}/g, '\n\n'), 'utf8');
}

function escapeRe(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }

// ── Manifest (.agent/.khora-ai.json) ──────────────────────────────────────
export function manifestPath(ctx) { return path.join(ctx.agentDir, '.khora-ai.json'); }

export function readManifest(ctx) {
  const p = manifestPath(ctx);
  if (!exists(p)) return { version: 1, profiles: {} };
  try { return JSON.parse(fs.readFileSync(p, 'utf8')); }
  catch { return { version: 1, profiles: {} }; }
}

export function writeManifest(ctx, m) {
  fs.writeFileSync(manifestPath(ctx), JSON.stringify(m, null, 2) + '\n', 'utf8');
}

// ── .gitignore managed block (union of all generated paths across profiles) ─
export function rewriteGitignore(ctx) {
  const m = readManifest(ctx);
  const paths = new Set();
  paths.add('.agent/.khora-ai.json');
  paths.add('.khora/'); // project-local tool home (headroom venv, etc.)
  // Bootstrapped design skill (impeccable) — installed per harness, never committed.
  for (const h of ['.claude', '.cursor', '.gemini', '.github']) paths.add(`${h}/skills/impeccable/`);
  for (const prof of Object.values(m.profiles)) {
    for (const g of (prof.generated ?? [])) paths.add(g);
  }
  const body =
    '# Generated AI provider wrappers — do NOT commit. Regenerate with:\n' +
    '#   node .agent/<profile>/installer/bin/khora-ai.mjs install all\n' +
    [...paths].sort().map((p) => '/' + p).join('\n');
  upsertManagedBlock(
    path.join(ctx.repoRoot, '.gitignore'),
    '# >>> khora-ai (generated) >>>',
    '# <<< khora-ai <<<',
    body,
  );
}

// ── git pre-commit hook (per-profile managed line + shared secret-scan) ─────
export function installGitHook(ctx) {
  const hooksDir = path.join(ctx.repoRoot, '.git', 'hooks');
  if (!exists(path.join(ctx.repoRoot, '.git'))) { log.warn('no .git dir — skipping git hook'); return; }
  fs.mkdirSync(hooksDir, { recursive: true });
  const hookPath = path.join(hooksDir, 'pre-commit');
  if (!exists(hookPath)) fs.writeFileSync(hookPath, '#!/bin/sh\n', 'utf8');

  const node = 'node';
  const binRel = `.agent/${ctx.profile}/installer/bin/khora-ai.mjs`;
  // Per-profile sync line.
  upsertManagedBlock(hookPath,
    `# >>> khora-ai:${ctx.profile} >>>`,
    `# <<< khora-ai:${ctx.profile} <<<`,
    `${node} ${binRel} sync --staged || exit 1`);
  // Shared secret-scan line (engine owns the canonical scanner; either profile installs it).
  upsertManagedBlock(hookPath,
    `# >>> khora-ai:secret-scan >>>`,
    `# <<< khora-ai:secret-scan <<<`,
    `${node} ${binRel} scan-secrets --staged || exit 1`);
  try { fs.chmodSync(hookPath, 0o755); } catch { /* windows: ignore */ }
  log.ok('git pre-commit hook wired (sync + secret-scan)');
}

export function removeGitHook(ctx) {
  const hookPath = path.join(ctx.repoRoot, '.git', 'hooks', 'pre-commit');
  removeManagedBlock(hookPath, `# >>> khora-ai:${ctx.profile} >>>`, `# <<< khora-ai:${ctx.profile} <<<`);
  // Only drop the shared secret-scan line if no profile remains installed.
  const m = readManifest(ctx);
  if (Object.keys(m.profiles).length === 0) {
    removeManagedBlock(hookPath, `# >>> khora-ai:secret-scan >>>`, `# <<< khora-ai:secret-scan <<<`);
  }
}

// ── staged-files helper (for sync --staged / scan-secrets) ─────────────────
export async function stagedFiles(ctx) {
  const { execFileSync } = await import('node:child_process');
  try {
    const out = execFileSync('git', ['diff', '--cached', '--name-only', '--diff-filter=ACM'],
      { cwd: ctx.repoRoot, encoding: 'utf8' });
    return out.split('\n').map((s) => s.trim()).filter(Boolean);
  } catch { return []; }
}
