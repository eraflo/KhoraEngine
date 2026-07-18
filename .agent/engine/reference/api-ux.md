# API / UX (SDK surface) — reference

Domain knowledge for the public `khora-sdk` surface. Consulted during Research (dispatch the `codebase-*`
subagents to apply it to concrete files).

## Scope
The `khora-sdk` surface that game developers touch: `EngineApp`, `GameWorld`, `Vessel`, `run_winit`/
`run_default`, the `prelude`, and the re-export discipline that keeps internals private.

## Key files
- SDK: `crates/khora-sdk/src/` (`lib.rs`, `engine.rs`, `game_world.rs`, `vessel.rs`, `traits.rs`, `run_default.rs`).
- I/O surface: `khora-io` (asset/serialization), `khora-plugins`.

## Principles
- **The SDK is a façade** — the Scheduler, `BudgetChannel`, `EnginePlugin`, and all `khora-*` internals stay
  hidden. Game code sees only the public API. Add a re-export only when game code genuinely needs the type.
- Prefer ergonomic builders (`Vessel::at(world, pos).with_component(..).build()`) and a clean `prelude`.
- `#[non_exhaustive]` on public enums/structs likely to grow; newtype IDs, no bare integers.
- `#![warn(missing_docs)]` is on — every public item needs rustdoc with a compiling `# Examples` block.
- Keep the gamedev profile ([`../../gamedev/`](../../gamedev/)) in sync: a public-API change should be
  reflected in the gamedev `sdk-guide.md` and skills.

Use codegraph to check what each re-export pulls in.

## Skills
- [`add-a-component`](../skills/add-a-component/SKILL.md) — when surfacing a new component to game code.
- [`build-and-test`](../skills/build-and-test/SKILL.md) — verify (`cargo build` + `cargo test --doc` for examples).
