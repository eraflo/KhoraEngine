// Provider wrapper generators. Each returns by recording every file it writes
// into `generated` (repo-root-relative posix paths), so install can gitignore
// them and uninstall can remove them.

import path from 'node:path';
import fs from 'node:fs';
import { writeFileRecording, copyTreeRecording, exists, log } from './core.mjs';

// ── Per-profile essentials baked into the thin routers ─────────────────────
function essentials(profile) {
  if (profile === 'gamedev') {
    return {
      title: 'Khora Engine — Game Developer (SDK) profile',
      identity:
        'You are building a GAME with the Khora Engine. You use ONLY the public `khora-sdk` API; ' +
        'engine internals (khora-control, khora-agents, khora-lanes, …) are off-limits.',
      rules: [
        'Use only the `khora-sdk` public surface (`prelude`, `EngineApp`, `GameWorld`, `Vessel`, `run_winit`).',
        'Never reach into `khora-*` internal crates — if the SDK does not expose it, ask, do not work around it.',
        'Math via `khora_sdk::prelude::math` — never raw `glam`. Log via `log::*`, never `println!`.',
        'Never embed secrets/keys in a game build or commit them to git.',
        'For any UI / design decision, use `/impeccable`.',
      ],
    };
  }
  return {
    title: 'Khora Engine — Engine Development profile',
    identity:
      'You are working ON Khora Engine — an experimental Rust game engine (SAA / CLAD). ' +
      'Precise, technical, concise Rust systems programmer. Reply in the user\'s language (FR/EN).',
    rules: [
      'Math via `khora_core::math` — never raw `glam`. Log via `log::*`, never `println!`.',
      'Never `unwrap()` on fallible GPU/IO. Never `std::thread::spawn` — concurrency goes through the DCC.',
      'Never bypass the `Lane` abstraction for hot-path work; agents implement only `Agent` + `Default`.',
      'Shaders are `.wgsl` files composed via `ShaderRegistry` — never inline WGSL strings.',
      'Never push to git or create PRs without explicit permission. Never commit secrets.',
    ],
  };
}

function canon(profile, file) { return `.agent/${profile}/${file}`; }

function frontmatterDescription(content, fallback) {
  const m = content.match(/^---\n[\s\S]*?\ndescription:\s*(.+)\n[\s\S]*?\n---/);
  return (m ? m[1] : fallback).replace(/^["']|["']$/g, '').trim();
}

// Router body shared by every provider (markdown). `importLine` lets providers
// that support @-imports (Claude, Gemini) pull the canonical docs directly.
function routerMarkdown(ctx, { withImports }) {
  const e = essentials(ctx.profile);
  const p = ctx.profile;
  const imports = withImports
    ? `\n## Canonical context (imported)\n\n@${canon(p, 'SOUL.md')}\n@${canon(p, 'RULES.md')}\n@${canon(p, 'index.md')}\n`
    : '';
  return `# ${e.title}

> Generated wrapper — do not edit. Single source of truth: \`${canon(p, '')}\`. Regenerate with
> \`node ${canon(p, 'installer/bin/khora-ai.mjs')} install all\`.

${e.identity}

## Read first
- [\`${canon(p, 'SOUL.md')}\`](${canon(p, 'SOUL.md')}) — identity, global map, routing.
- [\`${canon(p, 'RULES.md')}\`](${canon(p, 'RULES.md')}) — hard constraints, boundaries, permissions.
- [\`${canon(p, 'index.md')}\`](${canon(p, 'index.md')}) — route to docs, agents, skills.
- [\`${canon(p, 'security-privacy.md')}\`](${canon(p, 'security-privacy.md')}) — no dangerous code, never push secrets.

## Hard rules
${e.rules.map((r) => `- ${r}`).join('\n')}

## Tooling
Query the **codegraph** MCP before grepping. Context is compressed by **headroom**. Token-optimized
commands via **rtk**. For any design / UI-UX task, use **\`/impeccable\`**.
${imports}`;
}

// ── Claude Code ────────────────────────────────────────────────────────────
export function generateClaude(ctx, generated) {
  const root = ctx.repoRoot;
  // Router
  writeFileRecording(ctx, path.join(root, 'CLAUDE.md'), routerMarkdown(ctx, { withImports: true }), generated);
  // Copy agents + skills verbatim into the Claude discovery dirs.
  copyTreeRecording(ctx, path.join(ctx.profileDir, 'agents'), path.join(root, '.claude', 'agents'), generated);
  copyTreeRecording(ctx, path.join(ctx.profileDir, 'skills'), path.join(root, '.claude', 'skills'), generated);
  // settings.json with the doc-change + headroom hooks (merged idempotently).
  const settingsPath = path.join(root, '.claude', 'settings.json');
  let settings = {};
  if (exists(settingsPath)) { try { settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8')); } catch {} }
  settings.hooks = mergeClaudeHooks(settings.hooks ?? {}, ctx);
  writeFileRecording(ctx, settingsPath, JSON.stringify(settings, null, 2) + '\n', generated);
  log.ok('Claude Code wrappers (CLAUDE.md, .claude/agents, .claude/skills, .claude/settings.json)');
}

function mergeClaudeHooks(hooks, ctx) {
  const bin = canon(ctx.profile, 'installer/bin/khora-ai.mjs');
  const syncCmd = `node ${bin} sync --if-changed "$CLAUDE_TOOL_INPUT_FILE_PATH"`;
  const headroomCmd = `node ${bin} launch-headroom`;
  const owned = (c) => typeof c === 'string' && c.includes('khora-ai.mjs');
  const post = (hooks.PostToolUse ?? []).filter((g) => !(g.hooks ?? []).some((h) => owned(h.command)));
  post.push({ matcher: 'Write|Edit', hooks: [{ type: 'command', command: syncCmd }] });
  const start = (hooks.SessionStart ?? []).filter((g) => !(g.hooks ?? []).some((h) => owned(h.command)));
  start.push({ hooks: [{ type: 'command', command: headroomCmd }] });
  return { ...hooks, PostToolUse: post, SessionStart: start };
}

// ── Cursor ─────────────────────────────────────────────────────────────────
export function generateCursor(ctx, generated) {
  const rulesDir = path.join(ctx.repoRoot, '.cursor', 'rules');
  const e = essentials(ctx.profile);
  // Short always-apply index rule (kept well under the token tax).
  const index =
`---
description: Khora ${ctx.profile} — entry rule. Routes to the canonical .agent docs.
alwaysApply: true
---

${e.identity}

Hard rules:
${e.rules.map((r) => `- ${r}`).join('\n')}

Full rules in \`${canon(ctx.profile, 'RULES.md')}\`; map in \`${canon(ctx.profile, 'index.md')}\`.
Query codegraph before grepping. For design, use /impeccable.
`;
  writeFileRecording(ctx, path.join(rulesDir, '000-index.mdc'), index, generated);

  // Core docs as agent-decided rules carrying full content.
  for (const doc of ['SOUL.md', 'RULES.md', 'conventions.md', 'architecture.md', 'security-privacy.md']) {
    const src = path.join(ctx.profileDir, doc);
    if (!exists(src)) continue;
    const body = fs.readFileSync(src, 'utf8');
    const desc = `Khora ${ctx.profile} ${doc.replace('.md', '')} — load when relevant.`;
    const mdc = `---\ndescription: ${desc}\nalwaysApply: false\n---\n\n${body}`;
    writeFileRecording(ctx, path.join(rulesDir, `10-${doc.replace('.md', '')}.mdc`), mdc, generated);
  }
  // Agents + skills as manual/agent-decided rules.
  cursorMirror(ctx, path.join(ctx.profileDir, 'agents'), path.join(rulesDir, 'agents'), generated, 'agent');
  cursorMirror(ctx, path.join(ctx.profileDir, 'skills'), path.join(rulesDir, 'skills'), generated, 'skill');
  log.ok('Cursor wrappers (.cursor/rules/*.mdc)');
}

function cursorMirror(ctx, srcDir, destDir, generated, kind) {
  if (!exists(srcDir)) return;
  for (const entry of fs.readdirSync(srcDir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      // skills are folders containing SKILL.md
      const skill = path.join(srcDir, entry.name, 'SKILL.md');
      if (exists(skill)) emitCursorRule(ctx, skill, path.join(destDir, `${entry.name}.mdc`), generated, kind);
    } else if (entry.name.endsWith('.md')) {
      emitCursorRule(ctx, path.join(srcDir, entry.name), path.join(destDir, entry.name.replace('.md', '.mdc')), generated, kind);
    }
  }
}

function emitCursorRule(ctx, src, dest, generated, kind) {
  const content = fs.readFileSync(src, 'utf8');
  const desc = frontmatterDescription(content, `Khora ${ctx.profile} ${kind}`);
  const mdc = `---\ndescription: ${JSON.stringify(desc)}\nalwaysApply: false\n---\n\n${content}`;
  writeFileRecording(ctx, dest, mdc, generated);
}

// ── GitHub Copilot ─────────────────────────────────────────────────────────
export function generateCopilot(ctx, generated) {
  const root = ctx.repoRoot;
  writeFileRecording(ctx, path.join(root, '.github', 'copilot-instructions.md'),
    routerMarkdown(ctx, { withImports: false }), generated);
  // Per-agent instruction files with applyTo globs (best-effort domain mapping).
  const globs = {
    'graphics-rendering-expert': 'crates/khora-lanes/src/render_lane/**,crates/khora-infra/src/graphics/**',
    'physics-expert': 'crates/khora-infra/src/physics/**,crates/khora-lanes/src/physics_lane/**',
    'audio-expert': 'crates/khora-infra/src/audio/**,crates/khora-lanes/src/audio_lane/**',
    'ecs-data-expert': 'crates/khora-data/**',
    'control-gorna-expert': 'crates/khora-control/**',
    'editor-ui-ux': 'crates/khora-editor/**',
    'api-ux-expert': 'crates/khora-sdk/**',
    'gameplay-expert': 'src/**,examples/**',
    'scene-design-expert': 'src/**,assets/**',
  };
  const agentsDir = path.join(ctx.profileDir, 'agents');
  if (exists(agentsDir)) {
    for (const f of fs.readdirSync(agentsDir)) {
      if (!f.endsWith('.md')) continue;
      const name = f.replace('.md', '');
      const applyTo = globs[name];
      if (!applyTo) continue;
      const content = fs.readFileSync(path.join(agentsDir, f), 'utf8').replace(/^---[\s\S]*?---\n/, '');
      const out = `---\napplyTo: "${applyTo}"\n---\n\n${content}`;
      writeFileRecording(ctx, path.join(root, '.github', 'instructions', `${name}.instructions.md`), out, generated);
    }
  }
  log.ok('GitHub Copilot wrappers (.github/copilot-instructions.md, .github/instructions/*)');
}

// ── Gemini CLI ─────────────────────────────────────────────────────────────
export function generateGemini(ctx, generated) {
  const root = ctx.repoRoot;
  writeFileRecording(ctx, path.join(root, 'GEMINI.md'), routerMarkdown(ctx, { withImports: true }), generated);
  const settings = {
    context: { fileName: ['GEMINI.md'] },
    // Gemini reads the canonical docs through the @imports in GEMINI.md.
    _khoraAi: { profile: ctx.profile, source: canon(ctx.profile, '') },
  };
  writeFileRecording(ctx, path.join(root, '.gemini', 'settings.json'), JSON.stringify(settings, null, 2) + '\n', generated);
  log.ok('Gemini CLI wrappers (GEMINI.md, .gemini/settings.json)');
}

// ── Universal AGENTS.md (cross-tool standard) ──────────────────────────────
export function generateAgentsMd(ctx, generated) {
  writeFileRecording(ctx, path.join(ctx.repoRoot, 'AGENTS.md'),
    routerMarkdown(ctx, { withImports: false }), generated);
  log.ok('AGENTS.md (universal router)');
}

export const PROVIDERS = {
  claude: generateClaude,
  cursor: generateCursor,
  copilot: generateCopilot,
  gemini: generateGemini,
};
