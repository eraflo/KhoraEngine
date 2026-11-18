# Khora SDK — Conventions (gamedev profile)

How to structure a game built with the Khora Engine. Pairs with [`RULES.md`](./RULES.md),
[`reference/ergon.md`](./reference/ergon.md) and [`sdk-guide.md`](./sdk-guide.md).

---

## Project structure
```text
<project>/
├── project.json            # name, engine_version
├── assets/
│   ├── scenes/             # *.kscene (default.kscene is written on first open)
│   ├── scripts/            # **/*.erg — the only folder compiled
│   ├── textures/ meshes/ audio/ shaders/
│   └── …                   # prefabs (*.kprefab) wherever they are saved
├── Cargo.toml, src/        # only with native code (hub: Add Native Code)
└── dist/<target>/          # Build Game output — generated
```
- Scripts: one module per concern (`player.erg`, `combat/damage.erg`); shared components and events in
  modules others `import`.
- Native code: `src/main.rs` holds `main()` (`env_logger`, the `#[global_allocator]`, `run_winit` and its
  bootstrap closure); the `EngineApp` type in its own module; a family in a module of its own.

## Patterns
- Gameplay state lives in Ergon `component`s and behavior fields, so scenes and saves keep it.
- One `EngineApp` type; it caches handles (`Arc<Mutex<InputMap>>`, `SharedTime`, `Arc<Activation>`) in
  `setup` — `update` has no `runtime`.
- Resources a family reads are inserted in `provide`, never in `setup`.
- Action names are `const` strings in Rust, the same literals in scripts.
- Move entities with `update_transform`, or `sync_global_transform` after `get_transform_mut`.
- `use khora_sdk::prelude::*;` then name `ecs::…`, `materials::…`, `math::…`, or import those modules' items.

## Naming
- Rust: `snake_case` functions/modules, `PascalCase` types, `SCREAMING_SNAKE_CASE` consts.
- Ergon: `PascalCase` behaviors, components, systems, events, functions and lifecycle members
  (`Update`, `OnSpawn`); `camelCase` or lower-case fields; action names `area.verb` (`player.jump`).

## Errors & logging
- `anyhow::Result` in `main`; `?` for setup and I/O. Never `unwrap()` on asset, device or file results.
- `log::{info,warn,error}` only — never `println!`. Initialize `env_logger` in `main` (`RUST_LOG=info`).
- Scripts log with `Log`, `Warn`, `Error`; refusals show in the editor's Problems tab.

## Math
- Rust: `khora_sdk::prelude::math` (`Vec3`, `Quaternion`, `Mat4`, `LinearRgba`). Right-handed, Y-up.
  Never raw `glam`. Ergon: `Vec3(…)`, `Quat(…)`, `Color(…)`.

## Docs and comments
- Contract first: say what holds, not how it came to be. No plan-step labels in code comments.

## UI / design
- For HUD, menus, and any visual decision, use **`/impeccable`** (audit / critique / polish).

## Verify
- Play in the editor; for native code, `cargo build` then `cargo run`. The window opens, the scene
  renders, scripts compile (no Problems), input works.
