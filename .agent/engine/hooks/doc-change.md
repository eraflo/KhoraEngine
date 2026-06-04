# Hook — doc-change (auto-reinstall wrappers)

**Purpose:** when this profile's AI docs change, regenerate the provider wrappers automatically, so the
copies in `.claude/`, `.cursor/`, `.gemini/`, etc. never drift from the canonical `.agent/engine/` source.

The installer wires this hook into each provider it installs. You do not run it by hand.

## Triggers
- **In-session (Claude Code / Gemini CLI):** a `PostToolUse` hook matching `Write|Edit`. After any file
  write, it runs:
  ```
  node .agent/engine/installer/bin/khora-ai.mjs sync --if-changed "$CLAUDE_TOOL_INPUT_FILE_PATH"
  ```
  `sync --if-changed <path>` is a no-op unless the touched path is under `.agent/engine/`; if it is, it
  regenerates the providers recorded in the manifest (`.agent/.khora-ai.json`).
- **Cross-editor:** a managed block in `.git/hooks/pre-commit` runs `khora-ai.mjs sync` whenever
  `.agent/engine/**` is staged — so edits from any editor propagate before commit.

## Scope
Engine wrappers regenerate only on changes to `.agent/engine/**`. The gamedev profile has its own hook
scoped to `.agent/gamedev/**`. Installing one profile's wrappers wires only that profile's hook.

## Edit the source, not the copy
The generated wrappers are gitignored and overwritten on every sync. Make changes in `.agent/engine/`.
