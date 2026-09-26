#!/usr/bin/env node
// khora-ai — generate provider AI wrappers from the canonical .agent/<profile>
// docs, wire the doc-change + secret-scan hooks, and gitignore every artifact.
// Zero-dependency Node ESM. Profile (engine|gamedev) is derived from this path.

import path from 'node:path';
import fs from 'node:fs';
import {
  makeContext, log, exists,
  readManifest, writeManifest, rewriteGitignore,
  installGitHook, removeGitHook, stagedFiles,
} from '../lib/core.mjs';
import { PROVIDERS, generateAgentsMd } from '../lib/providers.mjs';
import { bootstrapTools } from '../lib/tooling.mjs';
import { scanSecrets } from '../lib/secrets.mjs';
import { sweep } from '../lib/sweep.mjs';

const ALL = Object.keys(PROVIDERS);

async function main() {
  const ctx = makeContext();
  const [cmd, ...rest] = process.argv.slice(2);
  const flags = new Set(rest.filter((a) => a.startsWith('--')));
  const args = rest.filter((a) => !a.startsWith('--'));

  switch (cmd) {
    case 'install':      return cmdInstall(ctx, args, flags);
    case 'sync':         return cmdSync(ctx, rest, flags);
    case 'uninstall':    return cmdUninstall(ctx, args);
    case 'scan-secrets': return cmdScanSecrets(ctx, flags);
    case 'list':         return cmdList(ctx);
    case 'sweep':        return cmdSweep(ctx, rest);
    case 'help': case undefined: return usage();
    default: log.err(`unknown command: ${cmd}`); usage(); process.exit(2);
  }
}

function resolveProviders(args) {
  if (args.length === 0 || args[0] === 'all') return [...ALL];
  const bad = args.filter((a) => !ALL.includes(a));
  if (bad.length) { log.err(`unknown provider(s): ${bad.join(', ')} (valid: ${ALL.join(', ')}, all)`); process.exit(2); }
  return args;
}

// Regenerate wrappers for a set of providers (no tooling). Returns generated paths.
function generateFor(ctx, providers) {
  const generated = [];
  generateAgentsMd(ctx, generated);             // universal router, always
  for (const p of providers) PROVIDERS[p](ctx, generated);
  // Persist manifest, then derive gitignore + git hook from it.
  const m = readManifest(ctx);
  m.profiles[ctx.profile] = { ...(m.profiles[ctx.profile] ?? {}), providers, generated };
  writeManifest(ctx, m);
  rewriteGitignore(ctx);
  installGitHook(ctx);
  return generated;
}

function cmdInstall(ctx, args, flags) {
  const providers = resolveProviders(args);
  log.step(`Installing ${ctx.profile} wrappers: ${providers.join(', ')}`);
  // Bootstrap tooling FIRST (rtk, codegraph, impeccable) so the generated wrappers
  // can reference tools that are actually present.
  bootstrapTools(ctx, { noTools: flags.has('--no-tools') });
  const generated = generateFor(ctx, providers);
  log.step(`Done — ${generated.length} artifacts generated and gitignored.`);
  log.info('Edit the canonical docs in .agent/' + ctx.profile + '/ — wrappers regenerate on change.');
}

async function cmdSync(ctx, rest, flags) {
  const m = readManifest(ctx);
  const entry = m.profiles[ctx.profile];
  if (!entry || !entry.providers?.length) return; // nothing installed for this profile

  const prefix = `.agent/${ctx.profile}/`;
  // --if-changed <path>: no-op unless the path is under this profile's docs.
  if (flags.has('--if-changed')) {
    const i = rest.indexOf('--if-changed');
    const raw = rest[i + 1];
    if (!raw) return;
    const relPath = path.relative(ctx.repoRoot, path.resolve(ctx.repoRoot, raw)).split(path.sep).join('/');
    if (!relPath.startsWith(prefix) || relPath.startsWith(prefix + 'installer/')) return;
  }
  // --staged: no-op unless staged files touch this profile's docs.
  if (flags.has('--staged')) {
    const touched = (await stagedFiles(ctx)).some((f) => f.startsWith(prefix) && !f.startsWith(prefix + 'installer/'));
    if (!touched) return;
  }
  log.step(`Syncing ${ctx.profile} wrappers (${entry.providers.join(', ')})`);
  generateFor(ctx, entry.providers);
  // Re-stage regenerated wrappers? They are gitignored, so nothing to stage.
}

function cmdUninstall(ctx, args) {
  const m = readManifest(ctx);
  const entry = m.profiles[ctx.profile];
  if (!entry) { log.info('nothing installed for ' + ctx.profile); return; }
  // Delete every currently-generated artifact for this profile.
  for (const rel of entry.generated ?? []) {
    const abs = path.join(ctx.repoRoot, rel);
    try { fs.rmSync(abs, { recursive: true, force: true }); } catch {}
  }
  const remaining = args[0] && args[0] !== 'all' ? entry.providers.filter((p) => !args.includes(p)) : [];
  if (remaining.length) {
    log.step(`Removed; regenerating remaining: ${remaining.join(', ')}`);
    generateFor(ctx, remaining);
  } else {
    delete m.profiles[ctx.profile];
    writeManifest(ctx, m);
    rewriteGitignore(ctx);
    removeGitHook(ctx);
    log.step(`Uninstalled all ${ctx.profile} wrappers.`);
  }
}

async function cmdScanSecrets(ctx, flags) {
  const ok = await scanSecrets(ctx, { staged: flags.has('--staged') });
  process.exit(ok ? 0 : 1);
}

function cmdList(ctx) {
  const m = readManifest(ctx);
  const e = m.profiles[ctx.profile];
  console.log(`Profile: ${ctx.profile}`);
  console.log(`Repo root: ${ctx.repoRoot}`);
  console.log(`Installed providers: ${e?.providers?.join(', ') || '(none)'}`);
  console.log(`Generated artifacts: ${e?.generated?.length || 0}`);
}

// Prunes <repo>/target (see lib/sweep.mjs). Always exits 0: it runs at the
// start of every agent session and a cleanup must never block one.
async function cmdSweep(ctx, rest) {
  // Strict parsing: a deletion must never follow from a typo. A value that is
  // not a plain non-negative number falls back to the default; an unknown
  // argument (a mistyped `--dry-run`, say) stops the sweep before it removes
  // anything.
  const opts = { maxAgeDays: 3, maxGb: 20, dryRun: false, force: false };
  const number = (s) => (typeof s === 'string' && /^\d+(\.\d+)?$/.test(s) ? Number(s) : undefined);
  for (let i = 0; i < rest.length; i++) {
    const arg = rest[i];
    if (arg === '--dry-run') opts.dryRun = true;
    else if (arg === '--force') opts.force = true;
    else if (arg === '--days' || arg === '--max-gb') {
      const next = rest[i + 1];
      if (next !== undefined && !next.startsWith('--')) {
        i++;
        const n = number(next);
        if (n !== undefined) opts[arg === '--days' ? 'maxAgeDays' : 'maxGb'] = n;
      }
    } else {
      console.log(`target sweep: unknown argument "${arg}", nothing removed`);
      return;
    }
  }
  const r = await sweep(path.join(ctx.repoRoot, 'target'), {
    maxAgeDays: opts.maxAgeDays,
    maxGb: opts.maxGb,
    dryRun: opts.dryRun,
    ...(opts.force ? { minIntervalHours: 0 } : {}),
  });
  if (r.skipped) {
    console.log('target sweep: skipped, the last one ran recently (--force to run anyway)');
    return;
  }
  const gb = (b) => (b / 1024 ** 3).toFixed(2);
  const verb = opts.dryRun ? 'would remove' : 'removed';
  const full = r.fullClean ? ', debug/release profiles over the limit' : '';
  const errs = r.errors.length ? `, ${r.errors.length} could not be removed (in use?)` : '';
  console.log(`target sweep: ${verb} ${r.removed.length} dir(s)${full}; ${gb(r.bytesBefore)} → ${gb(r.bytesAfter)} GiB${errs}`);
}

function usage() {
  const bin = `node .agent/<profile>/installer/bin/khora-ai.mjs`;
  console.log(`khora-ai — provider AI wrapper generator (profile derived from path)

Usage:
  ${bin} install <provider|all> [--no-tools]   Generate wrappers + wire hooks + gitignore
  ${bin} sync [--if-changed <path>|--staged]    Regenerate installed providers
  ${bin} uninstall <provider|all>               Remove wrappers + gitignore/hook entries
  ${bin} scan-secrets --staged                  Secret scan (used by pre-commit)
  ${bin} list                                   Show install state
  ${bin} sweep [--days N] [--max-gb G] [--dry-run] [--force]
                                                Prune target/: stale incremental caches, whole profiles past G GiB

Providers: ${ALL.join(', ')}, all
Examples:
  ${bin} install claude
  ${bin} install all
  ${bin} install all --no-tools`);
}

main().catch((e) => { log.err(e.stack || e.message); process.exit(1); });
