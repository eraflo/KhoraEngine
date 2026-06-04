# Hook — doc-change (auto-reinstall wrappers)

**Purpose:** when the gamedev AI docs change, regenerate the provider wrappers automatically, so the copies
in `.claude/`, `.cursor/`, `.gemini/`, etc. never drift from the canonical `.agent/gamedev/` source.

Installed with the wrappers; you don't run it by hand.

## Triggers
- **In-session (Claude / Gemini):** `PostToolUse` matching `Write|Edit` runs
  `node .agent/gamedev/installer/bin/khora-ai.mjs sync --if-changed "$CLAUDE_TOOL_INPUT_FILE_PATH"` — a
  no-op unless the touched path is under `.agent/gamedev/`.
- **Cross-editor:** a managed block in `.git/hooks/pre-commit` runs `sync` when `.agent/gamedev/**` is staged.

## Scope
Gamedev wrappers regenerate only on changes to `.agent/gamedev/**` (the engine profile is independent).

Edit the canonical source in `.agent/gamedev/` — the generated wrappers are gitignored and overwritten.
