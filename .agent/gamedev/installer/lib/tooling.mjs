// Best-effort, idempotent, non-fatal tooling bootstrap: rtk, codegraph,
// impeccable. Failures only warn — they never block an install.
// Project-local tools live in a gitignored `.khora/` folder.

import { execFileSync, execSync } from 'node:child_process';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import { log, exists } from './core.mjs';

const WIN = process.platform === 'win32';

function has(cmd) {
  try { execFileSync(WIN ? 'where' : 'which', [cmd], { stdio: 'ignore' }); return true; }
  catch { return false; }
}
function run(cmd, args, { timeout = 120000, shell = false } = {}) {
  execFileSync(cmd, args, { stdio: 'inherit', timeout, shell });
}
function khoraDir(ctx) { return path.join(ctx.repoRoot, '.khora'); }

export function bootstrapTools(ctx, { noTools }) {
  if (noTools) { log.info('tooling bootstrap skipped (--no-tools)'); return; }
  log.step('Tooling bootstrap (best-effort, project-local in .khora/)');
  ensureKhoraDir(ctx);
  rtk();
  ripgrep();
  codegraph();
  impeccable();
}

function ensureKhoraDir(ctx) {
  const dir = khoraDir(ctx);
  try {
    fs.mkdirSync(dir, { recursive: true });
    // Belt-and-braces: the folder ignores its own contents even if the managed
    // .gitignore block is missing.
    const gi = path.join(dir, '.gitignore');
    if (!exists(gi)) fs.writeFileSync(gi, '*\n', 'utf8');
  } catch (e) { log.warn(`.khora: ${e.message}`); }
}

// ── rtk: detect, else install from rtk-ai/rtk + ensure PATH ────────────────
function rtk() {
  try {
    if (has('rtk')) {
      try { run('rtk', ['--version']); } catch {}
      log.ok('rtk present');
      return;
    }
    log.info('rtk not found — installing from github.com/rtk-ai/rtk…');
    if (WIN) {
      if (has('cargo')) {
        try { run('cargo', ['install', '--git', 'https://github.com/rtk-ai/rtk'], { timeout: 600000 }); log.ok('rtk installed via cargo (~/.cargo/bin, already on PATH)'); }
        catch { log.warn('cargo install rtk failed — download the prebuilt x86_64-pc-windows-msvc binary from https://github.com/rtk-ai/rtk/releases'); }
      } else {
        log.warn('no cargo — install rtk manually: prebuilt binary from https://github.com/rtk-ai/rtk/releases (add its folder to PATH)');
      }
    } else {
      try {
        run('sh', ['-c', 'curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/refs/heads/master/install.sh | sh'], { timeout: 300000 });
        log.ok('rtk installed to ~/.local/bin');
        ensureLocalBinOnPath();
      } catch { log.warn('rtk install script failed — try `brew install rtk` or `cargo install --git https://github.com/rtk-ai/rtk`'); }
    }
  } catch (e) { log.warn(`rtk: ${e.message}`); }
}

function ensureLocalBinOnPath() {
  try {
    const home = os.homedir();
    const localBin = path.join(home, '.local', 'bin');
    if ((process.env.PATH || '').split(':').includes(localBin)) return;
    const line = 'export PATH="$HOME/.local/bin:$PATH"  # added by khora-ai';
    let touched = false;
    for (const rc of ['.zshrc', '.bashrc', '.profile']) {
      const rcPath = path.join(home, rc);
      if (!exists(rcPath)) continue;
      const content = fs.readFileSync(rcPath, 'utf8');
      if (/\.local\/bin/.test(content)) continue;
      fs.appendFileSync(rcPath, `\n${line}\n`);
      touched = true;
    }
    if (touched) log.warn('added ~/.local/bin to your shell PATH — restart your shell (or `source ~/.zshrc`)');
  } catch (e) { log.warn(`rtk PATH: ${e.message}`); }
}

// ── ripgrep: rtk delegates search to `rg`; ensure it's on PATH ─────────────
function ripgrep() {
  try {
    if (has('rg')) { log.ok('ripgrep (rg) present'); return; }
    if (has('cargo')) {
      log.info('ripgrep (rg) not found — rtk needs it; installing via cargo…');
      try { run('cargo', ['install', 'ripgrep'], { timeout: 600000 }); log.ok('ripgrep installed via cargo (~/.cargo/bin)'); }
      catch { log.warn('cargo install ripgrep failed — install rg manually (winget install BurntSushi.ripgrep.MSVC / brew install ripgrep)'); }
    } else {
      log.warn('ripgrep (rg) missing and no cargo — rtk falls back to slow exec. Install rg: winget install BurntSushi.ripgrep.MSVC / brew install ripgrep');
    }
  } catch (e) { log.warn(`ripgrep: ${e.message}`); }
}

// ── codegraph: manages its own .codegraph/ index ───────────────────────────
function codegraph() {
  try {
    if (!has('codegraph')) {
      // npm is a .cmd on Windows (execFile needs shell). The MCP server in
      // .mcp.json points at this CLI (`codegraph serve --mcp`).
      log.info('codegraph CLI not found — installing @colbymchenry/codegraph globally…');
      try { execSync('npm install -g @colbymchenry/codegraph', { stdio: 'inherit', timeout: 300000 }); log.ok('codegraph installed (npm -g)'); }
      catch { log.warn('codegraph install failed — `npm i -g @colbymchenry/codegraph` (agents fall back to grep)'); return; }
    }
    // Refresh the index. Never reinstall when present: reinstalling over the
    // running codegraph daemon EPERMs on Windows (locked node.exe).
    try { run('codegraph', ['index', '.'], { timeout: 300000 }); log.ok('codegraph index refreshed'); }
    catch { log.ok('codegraph present'); }
  } catch (e) { log.warn(`codegraph: ${e.message}`); }
}

// ── impeccable: design skill (writes provider files via npx; nothing to keep) ─
function impeccable() {
  try {
    log.info('installing impeccable design skill (npx impeccable skills install)…');
    // npx is a .cmd on Windows (needs the shell); `skills install` prompts
    // "Install … into N folder(s)? (Y/n)" → confirm via stdin. execSync runs
    // through the shell with a string command (avoids the DEP0190 shell+args warning).
    execSync('npx -y impeccable skills install', {
      stdio: ['pipe', 'inherit', 'inherit'], input: 'y\n', timeout: 180000,
    });
    log.ok('impeccable installed (/impeccable available)');
  } catch { log.warn('could not install impeccable — `npx impeccable skills install` (design authority for UI/UX)'); }
}
