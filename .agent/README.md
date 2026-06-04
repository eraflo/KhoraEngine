# Khora Engine — AI Documentation

This tree is the **single source of truth** for every AI coding agent that works in this repository.
It is provider-agnostic: you author the docs once here, then an installer generates the per-provider
wrappers (Claude Code, Cursor, GitHub Copilot, Gemini CLI) and keeps them in sync — without ever
duplicating the docs into git.

All content is written in **English**.

---

## Two profiles

| Profile | For | Lives in |
|---|---|---|
| **engine** | Developing the Khora Engine **itself** (contributors). Internals: CLAD, GORNA, crates, lanes, agents. | [`engine/`](./engine/index.md) → start at [`engine/index.md`](./engine/index.md) |
| **gamedev** | Building a **game with** the engine (SDK users). Public `khora-sdk` surface only. Standalone / publishable into downstream game projects. | [`gamedev/`](./gamedev/index.md) → start at [`gamedev/index.md`](./gamedev/index.md) |

Each profile contains:

- `SOUL.md` — the base **orchestrator** agent (identity, global map, routing).
- `RULES.md` — hard constraints, boundaries (do-not-touch), permission model.
- `index.md` / `index.yaml` — human + machine index (progressive disclosure — load on demand).
- domain docs (`conventions.md`, `architecture.md` / `sdk-guide.md`, `security-privacy.md`).
- `knowledge/` — persistent memory.
- `agents/` — specialist sub-agents (scoped tools).
- `skills/` — task workflows (SKILL.md).
- `hooks/` — doc-change, bootstrap, teardown, secret-scan.
- `installer/` — the Node CLI for that profile.

---

## Install the provider wrappers

Run from the repo root. The profile is derived from the installer's path.

### Engine (contributors)
```bash
node .agent/engine/installer/bin/khora-ai.mjs install claude     # Claude Code
node .agent/engine/installer/bin/khora-ai.mjs install cursor     # Cursor
node .agent/engine/installer/bin/khora-ai.mjs install copilot    # GitHub Copilot
node .agent/engine/installer/bin/khora-ai.mjs install gemini     # Gemini CLI
node .agent/engine/installer/bin/khora-ai.mjs install all        # all four
node .agent/engine/installer/bin/khora-ai.mjs install all --no-tools   # skip tooling bootstrap
```

### Gamedev (game developers)
```bash
node .agent/gamedev/installer/bin/khora-ai.mjs install claude    # …cursor | copilot | gemini | all
node .agent/gamedev/installer/bin/khora-ai.mjs install all
```

Also available: `npx khora-ai-engine install all` (via each installer's `package.json` bin), and
`cargo xtask ai install all` (convenience wrapper that shells out to node).

### Other commands
```bash
node .agent/<profile>/installer/bin/khora-ai.mjs sync            # regenerate installed providers
node .agent/<profile>/installer/bin/khora-ai.mjs uninstall all   # remove wrappers + gitignore/hook entries
node .agent/<profile>/installer/bin/khora-ai.mjs list            # show install state
```

---

## What the installer does

1. **Generates** thin per-provider routers (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`,
   `.github/copilot-instructions.md`) that carry basic identity + a few hard rules and **route into the
   profile index** — plus **copies** the skills/agents/docs into each provider's discovery folder
   (`.claude/`, `.cursor/rules/`, `.gemini/`, `.github/instructions/`).
2. **Wires hooks** into each provider:
   - a **doc-change** hook — editing `.agent/<profile>/**` re-runs that profile's installer (in-session via
     the provider's `PostToolUse`, cross-editor via a `git pre-commit` block);
   - a **secret-scan** `pre-commit` hook — blocks commits containing secrets;
   - a **headroom** `SessionStart` launch (context compression).
3. **Bootstraps tooling** (best-effort, `--no-tools` to skip): **rtk** (Rust Token Killer — detected, or
   installed from [rtk-ai/rtk](https://github.com/rtk-ai/rtk) and added to PATH if missing), **codegraph**,
   **headroom** (context compression — a Python venv under `.khora/`), and **impeccable** (the `/impeccable`
   design authority). Project-local tool state lives in a gitignored `.khora/` folder.
4. **Gitignores every generated artifact** via a managed block in `.gitignore`, and records the install in
   `.agent/.khora-ai.json` (per-machine, gitignored).

**The only committed AI docs are under `.agent/`.** Everything a provider needs is generated locally and
gitignored — so the docs are never duplicated across providers in git. After cloning, run the installer for
your provider(s).

> **Never edit the generated wrappers** (`CLAUDE.md`, `.claude/`, `.cursor/`, …). Edit the canonical source
> in `.agent/<profile>/`; the doc-change hook regenerates the rest.

### Notes
- This repo primarily uses the **engine** profile. The **gamedev** profile is self-contained and meant to be
  copied/seeded into downstream game projects (where it owns the root routers). Installing both profiles in
  one repo means the root routers and `.claude/` reflect the most recently synced profile.
- The doc-change hook is **scoped per profile**: engine wrappers regenerate only on `.agent/engine/**`
  changes, gamedev only on `.agent/gamedev/**`.
