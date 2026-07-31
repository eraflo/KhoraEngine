# Knowledge — SDK Context

## What you depend on
- A single crate: **`khora-sdk`** (Apache-2.0). Everything you need is re-exported from it or its `prelude`.
- The engine's internal crates (`khora-core/control/agents/lanes/data/infra/io/telemetry`) are
  implementation details — do not depend on them directly.

## SDK surface (entry points)
- `run_winit::<WinitWindowProvider, MyGame>(bootstrap)` — windowed entry.
- `run_default(...)` — packed-asset runtime entry (used by shipped builds via `khora-runtime`).
- Traits: `EngineApp`, `AgentProvider`, `PhaseProvider`, `WindowProvider`.
- `GameWorld`, `Vessel`, `spawn_cube_at`, `spawn_sphere`, `spawn_plane`.
- `prelude` modules: `ecs`, `materials`, `math`; plus `InputEvent`/`KeyCode`/`MouseButton`/`InputMap`.
- Backends to register: `WgpuRenderSystem`, `RapierPhysicsWorld`, `TaffyLayoutSystem`,
  `StandardTextRenderer`, `CpalAudioDevice` + `DefaultMixBus`.

## Commands
- `cargo build` — build your game.
- `cargo run` — run it (window opens, scene renders).
- `cargo run -p khora-editor` (engine repo) — author scenes/prefabs visually.

## Reference
- `examples/sandbox/src/main.rs` — a complete game using only the SDK.
