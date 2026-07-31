# Khora SDK — Conventions (gamedev profile)

How to structure a game built with the Khora Engine. Pairs with [`RULES.md`](./RULES.md) and
[`sdk-guide.md`](./sdk-guide.md).

---

## Project structure
- A game is its own Cargo binary crate depending on `khora-sdk`.
- `src/main.rs` — `main()` with `env_logger` + `run_winit`, the `#[global_allocator]`, and the backend
  bootstrap closure.
- Split large games into modules: `game/` (your `EngineApp` type + systems), `entities/`, `input.rs`,
  `scenes/`. Keep game logic out of `main.rs`.

## Patterns
- One game type implements `EngineApp + AgentProvider + PhaseProvider`. Keep per-frame state on it.
- Cache `Arc<Mutex<…>>` service handles (e.g. `InputMap`) in `setup`; `update` has no `runtime` argument.
- Centralize input action names as `const` strings so `bind` and `is_pressed` can't drift.
- Spawn through `Vessel` / spawn helpers; mutate via `GameWorld` methods (`get_transform_mut`,
  `sync_global_transform`). Call `sync_global_transform` after moving an entity you care about.
- Prefer `prelude::*` imports; reach for explicit `khora_sdk::Type` only when the prelude doesn't re-export it.

## Naming
- `snake_case` functions/modules, `PascalCase` types, `SCREAMING_SNAKE_CASE` consts (action names).

## Errors & logging
- `anyhow::Result` in `main`; `?` for setup/IO. Never `unwrap()` on asset/device/file results.
- `log::{info,warn,error}` only — never `println!`. Initialize `env_logger` in `main` (`RUST_LOG=info`).

## Math
- `khora_sdk::prelude::math` (`Vec3`, `Quaternion`, `Mat4`, `LinearRgba`). Right-handed, Y-up. Never raw `glam`.

## UI / design
- For HUD, menus, and any visual decision, use **`/impeccable`** (audit / critique / polish).

## Verify
- `cargo build` then `cargo run` your game. Confirm the window opens, the scene renders, and input works.
