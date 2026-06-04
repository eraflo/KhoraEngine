// Best-effort, idempotent, non-fatal tooling bootstrap: rtk, codegraph,
// headroom, impeccable. Failures only warn — they never block an install.

import { execFileSync, spawn } from 'node:child_process';
import net from 'node:net';
import { log } from './core.mjs';

const HEADROOM_PORT = 8787;

function has(cmd) {
  try {
    execFileSync(process.platform === 'win32' ? 'where' : 'which', [cmd], { stdio: 'ignore' });
    return true;
  } catch { return false; }
}

function run(cmd, args, { timeout = 120000 } = {}) {
  execFileSync(cmd, args, { stdio: 'inherit', timeout });
}

export function bootstrapTools(ctx, { noTools }) {
  if (noTools) { log.info('tooling bootstrap skipped (--no-tools)'); return; }
  log.step('Tooling bootstrap (best-effort)');
  rtk();
  codegraph();
  headroom();
  impeccable(ctx);
}

function rtk() {
  try {
    if (!has('rtk')) { log.warn('rtk not found — install the Rust Token Killer to enable token-optimized commands'); return; }
    try { run('rtk', ['ai']); log.ok('rtk ai configured'); }
    catch { log.warn('`rtk ai` not available on this rtk build — skipping'); }
  } catch (e) { log.warn(`rtk: ${e.message}`); }
}

function codegraph() {
  try {
    if (has('codegraph')) { try { run('codegraph', ['index', '.'], { timeout: 300000 }); log.ok('codegraph index refreshed'); } catch { log.ok('codegraph present'); } }
    else log.warn('codegraph CLI not found — agents will fall back to grep until the MCP server is registered');
  } catch (e) { log.warn(`codegraph: ${e.message}`); }
}

function headroom() {
  try {
    if (has('headroom')) { log.ok('headroom present (launched on session start)'); return; }
    log.info('installing headroom (npm i -g headroom-ai)…');
    try { run('npm', ['i', '-g', 'headroom-ai'], { timeout: 180000 }); log.ok('headroom installed'); return; }
    catch { /* fall through to pip */ }
    try { run('pip', ['install', 'headroom-ai[all]'], { timeout: 180000 }); log.ok('headroom installed (pip)'); }
    catch { log.warn('could not install headroom automatically — `npm i -g headroom-ai` or `pip install "headroom-ai[all]"`'); }
  } catch (e) { log.warn(`headroom: ${e.message}`); }
}

function impeccable(ctx) {
  try {
    log.info('installing impeccable design skill (npx impeccable skills install)…');
    run('npx', ['-y', 'impeccable', 'skills', 'install'], { timeout: 180000 });
    log.ok('impeccable installed (/impeccable available)');
  } catch { log.warn('could not install impeccable — `npx impeccable skills install` (design authority for UI/UX)'); }
}

// SessionStart hook target: ensure the headroom proxy is up. Fast + idempotent.
export function launchHeadroom() {
  if (!has('headroom')) return; // nothing to do
  const sock = net.connect(HEADROOM_PORT, '127.0.0.1');
  sock.setTimeout(300);
  sock.on('connect', () => { sock.destroy(); /* already running */ });
  sock.on('timeout', () => { sock.destroy(); start(); });
  sock.on('error', () => start());
  function start() {
    try {
      const child = spawn('headroom', ['proxy', '--port', String(HEADROOM_PORT)], { detached: true, stdio: 'ignore' });
      child.unref();
    } catch { /* best-effort */ }
  }
}
