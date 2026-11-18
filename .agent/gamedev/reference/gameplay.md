# Gameplay — reference

Domain knowledge for gameplay code. Consulted during Research (dispatch the `codebase-*` subagents to
apply it to concrete files). Gameplay is Ergon ([`ergon.md`](./ergon.md)); Rust holds what scripts
cannot do ([`../sdk-guide.md`](../sdk-guide.md)). Reference game: the engine's `examples/sandbox/`.

## Scope
Behaviors and the game's components, entity spawning and movement, input actions, physics bodies,
cameras, per-frame state, the native `EngineApp` lifecycle, which families run.

## Key API
- **Ergon**: `behavior`, `component`, `system`, `event`; `Update(float dt)`, `FixedUpdate`, `OnSpawn`;
  `this.SetPosition`, `e.Set/Add/Remove/Get/Has`, `Spawn`, `Raise`, `Pressed`/`JustPressed`, queries.
- **Engine components** a script writes: those `engine/components.erg` declares (`RigidBody`,
  `Collider`, `AudioSource`, …); in Rust, `prelude::ecs` (`Transform`, `GlobalTransform`, `Camera`,
  `Light`, `RigidBody`, `Collider`, `AudioSource`, `Name`, `Tag`, `Parent`/`Children`, `MeshRef`,
  `MaterialRef`, `ScriptValue`).
- **Rust**: `EngineApp` hooks (`setup`, `provide`, `update`, `startup_ready`); `GameWorld`
  (`add_declared_component`, `update_transform`, `query`); `Vessel` and the spawn helpers.
- **Input**: bind actions on `Arc<Mutex<InputMap>>` in `setup`; scripts read them by name.
- **Families**: `Arc<Activation>` — `activate`, `deactivate`, `scope(id, "Marker")`; scripts run only
  while `SCRIPT` is on.
- **Physics** collisions reach scripts as `Touched(Entity other)` / `Separated(Entity other)`.

## Rules
- Game state goes in components, so scenes and saves keep it; never in a Rust global.
- Components are declared in Ergon; `#[derive(Component)]` works in the engine's `khora-data` only.
- A script's world writes land at the frame's end; a Rust write is immediate.
- Rust: SDK-only (a family aside), math via `prelude::math`, `log::*`, no `unwrap()` on I/O, cache
  handles in `setup`.

## Skills
- [`write-a-script`](../skills/write-a-script/SKILL.md) — behaviors, components, systems, events.
- [`start-a-game`](../skills/start-a-game/SKILL.md) — the project, and its native code.
- [`spawn-entity`](../skills/spawn-entity/SKILL.md) — spawn from a script or from Rust.
- [`setup-input`](../skills/setup-input/SKILL.md) — bind actions.
- [`add-a-family`](../skills/add-a-family/SKILL.md) — a subsystem with levels to negotiate.

Verify with Play in the editor (or `cargo run`) — entities behave, input responds, Problems stays empty.
