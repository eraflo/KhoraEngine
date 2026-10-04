// Tests for the Claude Code hooks written by the installer: the SessionStart
// hook that runs `khora-ai.mjs sweep` (target/ cleanup) at every session start.
//
// CONTRACT: lib/providers.mjs must EXPORT `mergeClaudeHooks(hooks, ctx)`.
// The namespace import below keeps the file loadable while it is not exported,
// so each test fails on its own instead of the whole file failing to import.

import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import * as providers from './providers.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const profileDir = path.dirname(path.dirname(here)); // .agent/engine
const CTX = { profile: 'engine' };

const owned = (c) => typeof c === 'string' && c.includes('khora-ai.mjs');
const isSweep = (c) => owned(c) && /\bnode\b.*khora-ai\.mjs["']?\s+sweep\b/.test(c);

function merge(hooks) {
  assert.equal(typeof providers.mergeClaudeHooks, 'function', 'providers.mjs must export mergeClaudeHooks');
  return providers.mergeClaudeHooks(hooks, CTX);
}

function commands(groups) {
  return (groups ?? []).flatMap((g) => (g.hooks ?? []).map((h) => h.command));
}

const USER_START = { hooks: [{ type: 'command', command: 'echo user-session-hook' }] };

describe('mergeClaudeHooks — SessionStart sweep hook', () => {
  test('mergeClaudeHooks is exported', () => {
    assert.equal(typeof providers.mergeClaudeHooks, 'function');
  });

  test('adds a SessionStart command hook that runs `node <khora-ai.mjs> sweep`', () => {
    const out = merge({});
    const sweeps = (out.SessionStart ?? []).flatMap((g) => (g.hooks ?? []).filter((h) => isSweep(h.command)));
    assert.equal(sweeps.length, 1, `expected exactly one sweep hook, got ${JSON.stringify(out.SessionStart)}`);
    assert.equal(sweeps[0].type, 'command');
    assert.ok(sweeps[0].command.includes('.agent/engine/installer/bin/khora-ai.mjs'),
      'the hook must point at this profile\'s installer bin');
  });

  test('re-merging is idempotent: no duplicate sweep hook', () => {
    const once = merge({});
    const twice = merge(JSON.parse(JSON.stringify(once)));
    assert.deepEqual(twice, once);
    assert.equal(commands(twice.SessionStart).filter(isSweep).length, 1);
  });

  test('user-owned SessionStart hooks are kept', () => {
    const out = merge({ SessionStart: [USER_START] });
    const cmds = commands(out.SessionStart);
    assert.ok(cmds.includes('echo user-session-hook'), 'user hook must survive');
    assert.equal(cmds.filter(isSweep).length, 1);

    const again = merge(JSON.parse(JSON.stringify(out)));
    const cmds2 = commands(again.SessionStart);
    assert.equal(cmds2.filter((c) => c === 'echo user-session-hook').length, 1, 'user hook not duplicated');
    assert.equal(cmds2.filter(isSweep).length, 1);
  });

  test('a legacy khora-ai SessionStart hook is replaced by the sweep hook', () => {
    const legacy = { hooks: [{ type: 'command', command: 'node .agent/engine/installer/bin/khora-ai.mjs headroom-start' }] };
    const out = merge({ SessionStart: [legacy, USER_START] });
    const cmds = commands(out.SessionStart);
    assert.equal(cmds.filter(owned).length, 1, `only the sweep hook may be khora-owned: ${cmds}`);
    assert.equal(cmds.filter(isSweep).length, 1);
    assert.ok(cmds.includes('echo user-session-hook'));
  });

  test('the PostToolUse doc-sync hook is still installed alongside', () => {
    const out = merge({});
    const post = commands(out.PostToolUse);
    assert.equal(post.filter((c) => owned(c) && c.includes(' sync ')).length, 1);
  });

  test('unrelated hook events are preserved', () => {
    const stop = [{ hooks: [{ type: 'command', command: 'echo stop' }] }];
    const out = merge({ Stop: stop });
    assert.deepEqual(out.Stop, stop);
  });
});

// End to end through the exported generator: the written settings.json carries
// the sweep hook once, even after two installs, next to a user's own hook.
describe('generateClaude — settings.json', () => {
  test('writes one SessionStart sweep hook, keeps user hooks, stays idempotent across installs', (t) => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'khora-claude-'));
    t.after(() => fs.rmSync(root, { recursive: true, force: true, maxRetries: 5 }));
    const ctx = { repoRoot: root, profileDir, profile: 'engine' };
    const settingsPath = path.join(root, '.claude', 'settings.json');
    fs.mkdirSync(path.dirname(settingsPath), { recursive: true });
    fs.writeFileSync(settingsPath, JSON.stringify({ hooks: { SessionStart: [USER_START] } }, null, 2));

    providers.generateClaude(ctx, []);
    providers.generateClaude(ctx, []);

    const settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
    const cmds = commands(settings.hooks?.SessionStart);
    assert.equal(cmds.filter(isSweep).length, 1, `expected one sweep hook: ${JSON.stringify(settings.hooks)}`);
    assert.equal(cmds.filter((c) => c === 'echo user-session-hook').length, 1);
  });
});

// ── hook commands independent of the working directory ────────

const ENGINE_SWEEP = 'node "${CLAUDE_PROJECT_DIR}/.agent/engine/installer/bin/khora-ai.mjs" sweep';
const ENGINE_SYNC = 'node "${CLAUDE_PROJECT_DIR}/.agent/engine/installer/bin/khora-ai.mjs" sync --if-changed "$CLAUDE_TOOL_INPUT_FILE_PATH"';
const GAMEDEV_SYNC = 'node "${CLAUDE_PROJECT_DIR}/.agent/gamedev/installer/bin/khora-ai.mjs" sync --if-changed "$CLAUDE_TOOL_INPUT_FILE_PATH"';
const GAMEDEV_SYNC_LEGACY = 'node .agent/gamedev/installer/bin/khora-ai.mjs sync --if-changed "$CLAUDE_TOOL_INPUT_FILE_PATH"';

const group = (command, extra = {}) => ({ ...extra, hooks: [{ type: 'command', command }] });
const count = (cmds, c) => cmds.filter((x) => x === c).length;

describe('mergeClaudeHooks — $CLAUDE_PROJECT_DIR commands', () => {
  test('the SessionStart sweep hook is anchored on $CLAUDE_PROJECT_DIR', () => {
    const out = merge({});
    assert.deepEqual(commands(out.SessionStart), [ENGINE_SWEEP]);
  });

  test('the PostToolUse sync hook is anchored on $CLAUDE_PROJECT_DIR', () => {
    const out = merge({});
    assert.deepEqual(commands(out.PostToolUse), [ENGINE_SYNC]);
    assert.equal(out.PostToolUse[0].matcher, 'Write|Edit');
  });

  test('hooks from a previous install (relative paths) are replaced by the anchored ones', () => {
    const out = merge({
      SessionStart: [group('node .agent/engine/installer/bin/khora-ai.mjs sweep')],
      PostToolUse: [group('node .agent/engine/installer/bin/khora-ai.mjs sync --if-changed "$CLAUDE_TOOL_INPUT_FILE_PATH"', { matcher: 'Write|Edit' })],
    });
    assert.deepEqual(commands(out.SessionStart), [ENGINE_SWEEP]);
    assert.deepEqual(commands(out.PostToolUse), [ENGINE_SYNC]);
  });

  test('re-merging the anchored hooks is idempotent', () => {
    const once = merge({});
    const twice = merge(JSON.parse(JSON.stringify(once)));
    assert.deepEqual(twice, once);
    assert.deepEqual(commands(twice.SessionStart), [ENGINE_SWEEP]);
    assert.deepEqual(commands(twice.PostToolUse), [ENGINE_SYNC]);
  });
});

// ── each profile only owns its own hooks ──────────────────────

describe('mergeClaudeHooks (engine) — leaves the other profile\'s hooks alone', () => {
  test('an engine merge keeps the gamedev PostToolUse sync hook', () => {
    const out = merge({ PostToolUse: [group(GAMEDEV_SYNC, { matcher: 'Write|Edit' })] });
    const post = commands(out.PostToolUse);
    assert.equal(count(post, GAMEDEV_SYNC), 1, `the gamedev hook was removed: ${JSON.stringify(out.PostToolUse)}`);
    assert.equal(count(post, ENGINE_SYNC), 1);
  });

  test('an engine merge keeps a legacy (relative) gamedev hook too', () => {
    const out = merge({ PostToolUse: [group(GAMEDEV_SYNC_LEGACY, { matcher: 'Write|Edit' })] });
    assert.equal(count(commands(out.PostToolUse), GAMEDEV_SYNC_LEGACY), 1);
  });

  test('an engine merge keeps a gamedev SessionStart hook', () => {
    const gamedevStart = 'node "$CLAUDE_PROJECT_DIR/.agent/gamedev/installer/bin/khora-ai.mjs" list';
    const out = merge({ SessionStart: [group(gamedevStart)] });
    const cmds = commands(out.SessionStart);
    assert.equal(count(cmds, gamedevStart), 1);
    assert.equal(count(cmds, ENGINE_SWEEP), 1);
  });

  test('a khora-ai.mjs outside .agent/engine/installer/bin/ is not engine-owned and is kept', () => {
    const foreign = 'node tools/khora-ai.mjs sweep';
    const out = merge({ SessionStart: [group(foreign)] });
    assert.equal(count(commands(out.SessionStart), foreign), 1);
  });
});

// End to end through each profile's own generateClaude. The gamedev profile has
// its own copy of mergeClaudeHooks in .agent/gamedev/installer/lib/providers.mjs.
describe('generateClaude — engine and gamedev installs coexist', () => {
  const gamedevDir = path.join(path.dirname(profileDir), 'gamedev');
  const gamedevProviders = () => import(new URL('../../../gamedev/installer/lib/providers.mjs', import.meta.url));

  function tmpRepo(t) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'khora-claude-'));
    t.after(() => fs.rmSync(root, { recursive: true, force: true, maxRetries: 5 }));
    return root;
  }
  const readHooks = (root) =>
    JSON.parse(fs.readFileSync(path.join(root, '.claude', 'settings.json'), 'utf8')).hooks ?? {};

  test('a gamedev install keeps the engine SessionStart sweep and PostToolUse sync hooks', async (t) => {
    const root = tmpRepo(t);
    const settingsPath = path.join(root, '.claude', 'settings.json');
    fs.mkdirSync(path.dirname(settingsPath), { recursive: true });
    fs.writeFileSync(settingsPath, JSON.stringify({ hooks: {
      SessionStart: [group(ENGINE_SWEEP)],
      PostToolUse: [group(ENGINE_SYNC, { matcher: 'Write|Edit' })],
    } }, null, 2));
    const gamedev = await gamedevProviders();

    gamedev.generateClaude({ repoRoot: root, profileDir: gamedevDir, profile: 'gamedev' }, []);
    gamedev.generateClaude({ repoRoot: root, profileDir: gamedevDir, profile: 'gamedev' }, []);

    const hooks = readHooks(root);
    assert.equal(count(commands(hooks.SessionStart), ENGINE_SWEEP), 1,
      `the gamedev install removed the engine sweep hook: ${JSON.stringify(hooks)}`);
    assert.equal(count(commands(hooks.PostToolUse), ENGINE_SYNC), 1,
      `the gamedev install removed the engine sync hook: ${JSON.stringify(hooks)}`);
    const gamedevOwned = commands(hooks.PostToolUse).filter((c) => c.includes('.agent/gamedev/installer/bin/khora-ai.mjs'));
    assert.equal(gamedevOwned.length, 1, `the gamedev sync hook must appear once after two installs: ${gamedevOwned}`);
  });

  test('a gamedev install still replaces its own previous hook', async (t) => {
    const root = tmpRepo(t);
    const settingsPath = path.join(root, '.claude', 'settings.json');
    fs.mkdirSync(path.dirname(settingsPath), { recursive: true });
    fs.writeFileSync(settingsPath, JSON.stringify({ hooks: {
      SessionStart: [group(ENGINE_SWEEP)],
      PostToolUse: [group(GAMEDEV_SYNC_LEGACY, { matcher: 'Write|Edit' }), group(ENGINE_SYNC, { matcher: 'Write|Edit' })],
    } }, null, 2));
    const gamedev = await gamedevProviders();

    gamedev.generateClaude({ repoRoot: root, profileDir: gamedevDir, profile: 'gamedev' }, []);

    const post = commands(readHooks(root).PostToolUse);
    assert.equal(post.filter((c) => c.includes('.agent/gamedev/installer/bin/khora-ai.mjs')).length, 1,
      `exactly one gamedev hook expected: ${post}`);
    assert.equal(count(post, ENGINE_SYNC), 1, `the engine sync hook must survive: ${post}`);
  });

  test('engine → gamedev → engine installs leave each profile\'s hooks exactly once', async (t) => {
    const root = tmpRepo(t);
    const engineCtx = { repoRoot: root, profileDir, profile: 'engine' };
    const gamedevCtx = { repoRoot: root, profileDir: gamedevDir, profile: 'gamedev' };
    const gamedev = await gamedevProviders();

    providers.generateClaude(engineCtx, []);
    gamedev.generateClaude(gamedevCtx, []);
    providers.generateClaude(engineCtx, []);

    const hooks = readHooks(root);
    const start = commands(hooks.SessionStart);
    const post = commands(hooks.PostToolUse);
    assert.equal(count(start, ENGINE_SWEEP), 1, `engine sweep hook: ${JSON.stringify(hooks)}`);
    assert.equal(count(post, ENGINE_SYNC), 1, `engine sync hook: ${JSON.stringify(hooks)}`);
    assert.equal(post.filter((c) => c.includes('.agent/gamedev/installer/bin/khora-ai.mjs')).length, 1,
      `the engine reinstall removed the gamedev hook: ${JSON.stringify(hooks)}`);
  });
});

// ── hook commands must survive Claude Code's PowerShell hook shell ──
// On Windows, Claude Code runs a hook through Git Bash when it finds one and
// through PowerShell otherwise. PowerShell reads a bare `$CLAUDE_PROJECT_DIR`
// as an undefined variable, so `node "$CLAUDE_PROJECT_DIR/.agent/…"` becomes
// `node "/.agent/…"` (MODULE_NOT_FOUND); Claude Code itself warns about it.
// The braced `${CLAUDE_PROJECT_DIR}` works in both: bash expands it, and Claude
// Code rewrites it to `${env:CLAUDE_PROJECT_DIR}` for PowerShell.

describe('hook commands work under both hook shells', () => {
  const bareProjectDir = /\$CLAUDE_PROJECT_DIR\b/; // the pattern Claude Code warns on
  const gamedevProviders = () => import(new URL('../../../gamedev/installer/lib/providers.mjs', import.meta.url));

  test('engine hooks name the project root as ${CLAUDE_PROJECT_DIR}, never bare $CLAUDE_PROJECT_DIR', () => {
    const out = merge({});
    for (const c of [...commands(out.SessionStart), ...commands(out.PostToolUse)]) {
      assert.doesNotMatch(c, bareProjectDir, `breaks under the PowerShell hook shell: ${c}`);
      assert.ok(c.includes('${CLAUDE_PROJECT_DIR}/.agent/engine/installer/bin/khora-ai.mjs'), c);
    }
  });

  test('gamedev hooks name the project root as ${CLAUDE_PROJECT_DIR}, never bare $CLAUDE_PROJECT_DIR', async () => {
    const gamedev = await gamedevProviders();
    const out = gamedev.mergeClaudeHooks({}, { profile: 'gamedev' });
    for (const c of commands(out.PostToolUse)) {
      assert.doesNotMatch(c, bareProjectDir, `breaks under the PowerShell hook shell: ${c}`);
      assert.ok(c.includes('${CLAUDE_PROJECT_DIR}/.agent/gamedev/installer/bin/khora-ai.mjs'), c);
    }
  });
});
