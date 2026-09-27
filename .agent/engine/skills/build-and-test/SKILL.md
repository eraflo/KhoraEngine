---
name: build-and-test
description: Validates an engine change by compiling and running the workspace test suite. Use after any code edit, before declaring work complete, or when the user asks to build, test, lint, or check the engine.
---

# Build and test

## Commands

| Need | Command |
|---|---|
| **While iterating** — the tests you are working on | `cargo t -p <crate> [filter]` (nextest, `--all-features`) |
| **When the change is done** — what CI checks, stops at the first failure | `cargo gate` (fmt → clippy → nextest → doc tests) |
| Run an app | `cargo run -p sandbox` / `-p khora-editor` / `-p khora-hub` |

Scope a targeted run to the crate you changed. After a change, a run relinks the test binaries of every
crate it selects that depends on the change: `-p khora-lanes` relinks the lanes' tests (about 1.5 min),
a workspace-wide run relinks agents, SDK, editor and hub too (about 6 min).

## Rhythm — where the time goes

- While iterating, run **only** `cargo t -p <crate> [filter]`. No clippy, no full suite: clippy compiles
  in check mode and shares nothing with a test build, and the full suite is the gate's job.
- Run `cargo gate` **once**, when the phase is done — not after each fix. It costs 10–15 min after a
  change low in the stack (a test build of the whole workspace, plus about 2 min of doc tests that
  rustdoc recompiles on every run). A failing gate means fixing, checking the fix with `cargo t -p …`,
  then one more gate.
- Never wrap a build in `timeout` or a tool time limit: a killed link throws the whole build away. Run a
  long build in the background instead.
- Never build while a subagent builds: two cargo processes serialise on the lock.

## Definition of done
- `cargo gate` green: formatted, zero warnings, every test and doc test passing. Report the live counts.
- For GPU work, also run `cargo run -p sandbox` once and confirm a clean frame loop (no Vulkan validation errors, scene renders).

## When tests fail
Report the failure honestly with the output. Investigate the root cause before patching; never paper over
a failing test by ignoring it. If a step was skipped (e.g. you didn't run the sandbox), say so.
