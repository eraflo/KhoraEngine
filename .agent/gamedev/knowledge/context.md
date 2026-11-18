# Knowledge — SDK Context

## What a game is
- A **project** (hub-created, editor-opened): `project.json`, `assets/` (scenes, scripts, media).
- **Gameplay**: Ergon modules under `assets/scripts/` — behaviors, components, systems, events.
- **Native code** (optional): a Cargo binary depending on **`khora-sdk`** only. Everything else is
  re-exported by it (`khora_sdk::khora_core`, `khora_sdk::khora_data`, `khora_sdk::inventory`). A game's
  own family adds `khora-core`, `khora-macros` and `khora-agents`.

## SDK surface (entry points)
- `run_default()` — the packed-or-loose runtime (`khora-runtime`, and the hub's native template).
- `run_winit::<WinitWindowProvider, MyGame>(bootstrap)` — windowed entry for an `EngineApp`.
- Traits: `EngineApp` (requires `PhaseProvider`), `WindowProvider`.
- `open_game_assets::<MyGame>(runtime, dir)`, `scripts::mount(runtime, root)`.
- `GameWorld`, `Vessel`, `spawn_plane`, `spawn_cube_at`, `spawn_sphere`, `DeclaredComponentError`.
- `prelude` + modules `ecs`, `materials`, `math`; `InputEvent`, `KeyCode`, `MouseButton`, `SharedTime`.
- `Activation`, `Readiness`, `SceneLoadHub`, `SerializationService`, `SceneFile`, `SerializationGoal`,
  `LoadReport`, `fs`, `trust`.
- Backends to register: `WgpuRenderSystem` (+ its `graphics_device()`), `WgpuPipelineSystem` as
  `Arc<dyn PipelineSystem>`, `RapierPhysicsWorld`, `TaffyLayoutSystem`, `CpalAudioDevice` + `DefaultMixBus`.
  Text needs nothing: the boot installs the engine's shaper and rasterizer.

## Commands
- Engine repo: `cargo run -p khora-hub` (projects, engines), `cargo run -p khora-editor -- --project <path>`.
- Native code: `cargo build`, `cargo run`.
- Ship: editor *Build → Build Game…* → `<project>/dist/<target>/`.

## Reference
- `examples/sandbox/` (engine repo) — a native game on the SDK with one Ergon behavior (`Hover`).
- Engine book: `docs/src/reference/sdk.md`, `docs/src/reference/ergon.md`, `docs/src/tutorials/`.
