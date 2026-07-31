# Gameplay — reference

Domain knowledge for gameplay code on `khora-sdk`. Consulted during Research (dispatch the `codebase-*`
subagents to apply it to concrete files). Reference: `examples/sandbox/src/main.rs`,
[`../sdk-guide.md`](../sdk-guide.md).

## Scope
The `EngineApp` lifecycle (`new`/`setup`/`update`), entity spawning and movement, input actions, physics
bodies, cameras, and per-frame game state.

## Key API
- `Vessel::at(world, pos).with_component(..).with_rotation(..).build()`; `spawn_cube_at`, `spawn_sphere`, `spawn_plane`.
- `GameWorld`: `spawn`, `get_transform_mut`, `sync_global_transform`, `add_material`, `iter_entities`, `set_parent`.
- Input: bind on `Arc<Mutex<InputMap>>` (from `runtime.resources`) in `setup`; query `is_pressed("action")`
  in `update`; handle raw `InputEvent` for mouse motion.
- Components: `prelude::ecs::*` (`Transform`, `Camera`, `Light`, `RigidBody`, `Collider`, `AudioSource`).

## Rules
- SDK-only — never depend on internal `khora-*` crates. Math via `prelude::math`. Log via `log::*`.
- Cache service handles in `setup` (update has no `runtime` arg). Call `sync_global_transform` after moving entities.
- No `unwrap()` on fallible IO. Never commit secrets.

## Skills
- [`start-a-game`](../skills/start-a-game/SKILL.md) — scaffold the game + `EngineApp`.
- [`spawn-entity`](../skills/spawn-entity/SKILL.md) — spawn/configure entities via `Vessel`.
- [`setup-input`](../skills/setup-input/SKILL.md) — bind actions with `InputMap`.

Verify with `cargo run` — window opens, entities behave, input responds.
