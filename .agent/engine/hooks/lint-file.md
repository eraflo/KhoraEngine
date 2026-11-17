# Hook — lint-file (per-file convention checks)

**Purpose:** check every Rust file an agent writes against the conventions as soon as it is written, so
the agent fixes a finding before it moves on instead of a reviewer finding it later. Runs
`cargo xtask lint-file`: in-file order, file and function size, comments (plan labels, history, summary
length and verb, book citations) and the crate boundaries one file shows — the per-file checks of
`xtask/tests/layout.rs`, without compiling the engine.

The installer wires both triggers. You do not run the hook by hand; run `cargo xtask lint-file <files>`
yourself before a report (see `conventions.md`, "Before you write").

## Triggers
- **In-session (Claude Code):** a `PostToolUse` hook matching `Write|Edit`. After a file write, it runs:
  ```
  node .agent/engine/installer/bin/lint-file.mjs "$CLAUDE_TOOL_INPUT_FILE_PATH"
  ```
  The path is the argument when it is set, else `tool_input.file_path` of the JSON Claude Code writes on
  the hook's stdin. A file that is not `.rs`, or lies outside `crates/`, `xtask/` and `examples/`, passes
  untouched (exit 0).
- **Pre-commit:** a managed block in `.git/hooks/pre-commit` runs
  `node .agent/engine/installer/bin/lint-file.mjs --staged` on the staged `.rs` files (as they are in the
  working tree).

## Behavior
- **Clean → exit 0**, silent.
- **Finding, in-session → exit 2**, the findings on stderr as `path:line: check: message`, then what to do
  per check. Claude Code feeds them back to the agent, which fixes them.
- **Finding, pre-commit → exit 1**, and the commit is blocked.
- **lint-file cannot run** (xtask does not build, cargo is missing) → the hook says why and does not block:
  exit 1 in-session (shown to the user), exit 0 from the pre-commit (the gate's layout tests still check).
- A finding the ratchet allowlist (`xtask/tests/layout_allow.txt`) lists for that file is not reported,
  as in the layout tests.

## Which xtask runs
The prebuilt `target/debug/xtask(.exe)` of the default target runs directly when it is newer than every
file of `xtask/src/` and `xtask/Cargo.toml` — well under a second for one file. Otherwise the hook runs
`cargo xtask lint-file`, which rebuilds xtask into the default target first (`CARGO_TARGET_DIR` is
dropped for it), so the next run is direct again. The in-session hook allows 180 s for that rebuild.

## What it cannot check
Workspace-wide checks stay in `cargo t -p xtask`: whether a public function is named by a test of its
crate, duplicate public type names, orphan files, the crate graph. `cargo doc` and clippy run in the gate.
