# SDK surface

A curated map of the public `khora-sdk` surface. `khora-sdk` is the only crate a
game depends on; it re-exports the types it needs from internal crates so games
never reference those directly.

> This page names the surface and links to rustdoc for the exhaustive signatures.
> For the full method-by-method detail of any type below, follow its link to the
> [published rustdoc](https://eraflo.github.io/KhoraEngine/api/khora_sdk/index.html).
> For *how* to build a game, see [Your first game](../tutorials/your-first-game.md)
> and the [How-to guides](../how-to/index.md).

## The application traits

A Khora application implements three traits. The composite bound the engine
requires is `EngineApp + AgentProvider + PhaseProvider`.

| Trait | Role | rustdoc |
|---|---|---|
| [`EngineApp`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.EngineApp.html) | Application lifecycle — `window_config`, `new`, `setup`, `update`, `on_shutdown`, plus optional editor-overlay hooks. | link |
| [`AgentProvider`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.AgentProvider.html) | Register custom agents with the DCC. | link |
| [`PhaseProvider`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.PhaseProvider.html) | Insert or remove custom `ExecutionPhase`s. | link |
| [`WindowProvider`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.WindowProvider.html) | Abstracts the platform window backend (default: winit). | link |

### `EngineApp` — lifecycle

The methods the engine calls on your app type:

| Method | When |
|---|---|
| `window_config() -> WindowConfig` | Once, before window creation. |
| `new() -> Self` | Once, after window creation — no engine context yet. |
| `setup(&mut self, world: &mut GameWorld, runtime: &Runtime)` | Once, after engine init — spawn entities, cache handles. |
| `update(&mut self, world: &mut GameWorld, inputs: &[InputEvent])` | Every frame — game logic. |
| `on_shutdown(&mut self)` | Once, on exit (default no-op). |

The optional hooks `intercept_window_event`, `before_frame`, `before_agents`, and
`after_agents` exist so the editor can run an egui overlay around the engine's
frame loop. Most games leave them at their default no-ops.

`setup` and the optional hooks receive `&Runtime` — the engine's injection bundle
(see [Runtime](#runtime--the-injection-bundle) below).

### `AgentProvider` — register custom agents

```rust
fn register_agents(&self, dcc: &DccService, runtime: &mut Runtime);
```

Called once during boot. Empty for a vanilla game; this is where a custom AI,
scripting, or networking agent calls `dcc.register_agent(...)` or
`dcc.register_agent_for_mode(...)`. See
[Add an agent](../how-to/add-an-agent.md).

### `PhaseProvider` — custom execution phases

```rust
fn custom_phases(&self) -> Vec<ExecutionPhase> { Vec::new() }
fn removed_phases(&self) -> Vec<ExecutionPhase> { Vec::new() }
```

The built-in phases live in `khora_core::agent::ExecutionPhase`. Most games return
empty vectors. See the [`ExecutionPhase`](./glossary.md#executionphase--tickphase)
glossary entry.

## `run_winit` — the entry point

```rust
pub fn run_winit<W: WindowProvider, A: EngineApp>(
    bootstrap: impl FnOnce(&dyn KhoraWindow, &mut Runtime, &dyn Any) + Send + 'static,
) -> anyhow::Result<()>;
```

Opens a window through `W`, initializes the DCC, registers the default engine
services, runs your `bootstrap` closure, then enters the frame loop. It returns
when the window closes. The bootstrap closure receives:

- `&dyn KhoraWindow` — the platform window (use it to initialize the renderer).
- `&mut Runtime` — register your renderer and any custom backends/services here.
- `&dyn Any` — opaque handle to the native event loop (downcast if needed).

`WinitWindowProvider` is the default `WindowProvider`. `EngineCore` is the
underlying engine type, exposed for embedding `khora-sdk` inside another runtime
without `run_winit` (uncommon). `run_default` is the zero-config entry the
prebuilt `khora-runtime` binary uses — it auto-detects a `data.pack` and loads
the default scene.

## `Runtime` — the injection bundle

`Runtime` (re-exported from `khora-core`) is the engine-wide injection point.
Engine init builds one, wraps it in `Arc<Runtime>`, and from then on it is
immutable. It bundles three typed containers, each with a clear admission rule:

| Container | Holds | Examples |
|---|---|---|
| `runtime.services` | Concrete stateful objects with a rich business API. | `AssetService`, `SerializationService`, `TelemetryService`, `DccService` |
| `runtime.backends` | Concrete impls of abstract `khora-core` traits. | `dyn RenderSystem`, `dyn PhysicsProvider`, `dyn AudioDevice`, `dyn LayoutSystem` |
| `runtime.resources` | Long-lived shared state without a service-style API. | `GpuCache`, `InputMap`, viewport overrides |

Look up an entry with `runtime.services.get::<T>()`, `runtime.backends.get::<T>()`,
or `runtime.resources.get::<T>()` (each returns `Option<&T>`; `require::<T>()`
panics if absent). Per-frame state (current viewport, frame deltas, lane outputs)
does **not** live here — it flows through the `LaneBus` and `OutputDeck`.

> `Runtime` replaces the legacy single `ServiceRegistry`. The container API mirrors
> the old registry (`insert` / `get` / `require`), split three ways by admission
> criterion.

## `GameWorld` — the ECS facade

`GameWorld` is the safe public entry point for the ECS; it wraps the internal
`khora-data` `World`. The surface, grouped by what you do:

| Group | Methods |
|---|---|
| Lifecycle | `new`, `from_world` |
| Entities | `spawn`, `despawn`, `spawn_camera`, `spawn_entity`, `iter_entities` |
| Components | `add_component`, `remove_component`, `get_component`, `get_component_mut`, `get_transform`, `get_transform_mut` |
| Queries | `query::<...>()`, `query_mut::<...>()` |
| Transforms | `sync_global_transform`, `update_transform` |
| Hierarchy | `set_parent` |
| Assets | `add_mesh`, `add_material` |
| Internal | `inner_world`, `inner_world_mut` (low-level; prefer the wrapped surface) |

After mutating a `Transform`, call `sync_global_transform(entity)` (or use
`update_transform`, which mutates and syncs in one call) so the renderer sees the
updated `GlobalTransform`. Full signatures:
[`GameWorld`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.GameWorld.html).

## `Vessel` and the spawn helpers

`Vessel` is a builder over a freshly spawned entity. Every `Vessel` starts with a
`Transform` and a `GlobalTransform`.

- Construct: `Vessel::new(world)` (origin) or `Vessel::at(world, position)`.
- Build chain: `with_transform`, `at_position`, `with_rotation`, `with_scale`,
  `with_component` (chainable), `entity` (read the `EntityId` mid-build), `build`
  (finalize, sync `GlobalTransform`, return `EntityId`).

Primitive helpers are top-level functions returning a `Vessel`:
`spawn_plane(world, size, y)`, `spawn_cube_at(world, position, size)`,
`spawn_sphere(world, radius, segments, rings)`. See
[Spawn and transform](../how-to/spawn-and-transform.md). Full signatures:
[`Vessel`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.Vessel.html).

## The prelude

```rust
use khora_sdk::prelude::*;            // SDK + input + timing + assets
use khora_sdk::prelude::ecs::*;       // ECS components
use khora_sdk::prelude::math::*;      // Math types
use khora_sdk::prelude::materials::*; // Built-in materials
```

| Module | Contents |
|---|---|
| `prelude` | `WindowConfig`, `WindowIcon`, `PRIMARY_VIEWPORT`, `AssetHandle`, `AssetUUID`, `SaaTrackingAllocator`, `InputEvent`, `KeyCode`, `MouseButton`, `Time`, `SharedTime` |
| `prelude::ecs` | `EntityId`, `Transform`, `GlobalTransform`, `Camera`, `Light`, `LightType`, `DirectionalLight`, `PointLight`, `SpotLight`, `MeshRef`, `MaterialRef`, `RigidBody`, `Collider`, `BodyType`, `ColliderShape`, `AudioSource`, `Parent`, `Children`, `Name`, `Tag`, `Without`, `Component`, `ComponentBundle`, `ProjectionType`, `ProceduralMeshKind` |
| `prelude::materials` | `StandardMaterial`, `UnlitMaterial`, `EmissiveMaterial`, `WireframeMaterial` |
| `prelude::math` | `LinearRgba` and everything in `khora_core::math` (`Vec2/3/4`, `Mat3/4`, `Quaternion`, `Aabb`, …) |

The prelude is curated — adding to it is a deliberate decision. Full contents:
[`prelude`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/prelude/index.html).

## Input

Inputs arrive in `update` as `&[InputEvent]`. `InputEvent` and `KeyCode` /
`MouseButton` are re-exported at the crate root and in the prelude. `key_code`
values follow the [W3C UI Events](https://www.w3.org/TR/uievents-code/) code names
(`"KeyW"`, `"Space"`, `"Escape"`). See [Map input](../how-to/map-input.md) and the
[`InputEvent`](https://eraflo.github.io/KhoraEngine/api/khora_core/platform/enum.InputEvent.html)
rustdoc for the full variant list.

## Engine modes

`EngineMode` (re-exported from `khora-control`) gates **which agents run** each
frame. The base engine knows only `EngineMode::Playing`; other modes are injected
by plugins (an app *may* register mode-scoped agents through `DccService::register_agent_for_mode`; nothing in the engine or the editor does today). It is distinct
from `PlayMode` (`Editing` / `Playing` / `Paused`), the editor's own UI-state enum
re-exported from `khora_core::ui::editor`.

## SDK re-exports

`khora-sdk` re-exports types from internal crates so games depend on the SDK
alone. The major groups:

| Group | Re-exported types (selection) |
|---|---|
| Engine + ECS | `EngineCore`, `GameWorld`, `Vessel`, `spawn_*` |
| App traits | `EngineApp`, `AgentProvider`, `PhaseProvider`, `WindowProvider` |
| Bootstrap | `run_winit`, `run_default`, `WinitAppRunner`, `WinitWindowProvider` |
| Window | `WindowConfig`, `WindowIcon`, `PRIMARY_VIEWPORT` |
| Runtime / control | `Runtime`, `Services`, `Backends`, `Resources`, `DccService`, `DccConfig`, `EngineMode`, `EngineContext`, `AgentRegistry` |
| Core types | `ExecutionPhase`, `ExecutionTiming`, `AgentId`, `AgentStatus`, `StrategyId`, `AgentImportance` |
| Telemetry | `TelemetryService`, `TelemetryEvent`, `MonitoredResourceType`, `MetricsRegistry`, `MonitorRegistry` |
| Monitors | `GpuMonitor`, `MemoryMonitor` |
| Backends + traits | `WgpuRenderSystem`, `RenderSystem`, `PipelineSystem`, `RapierPhysicsWorld`, `PhysicsProvider`, `CpalAudioDevice`, `AudioDevice`, `TaffyLayoutSystem`, `LayoutSystem` |
| Scene I/O | `SerializationService`, `SceneFile`, `SerializationGoal` |
| Assets | `AssetService`, `AssetIo`, `FileLoader`, `PackLoader`, `PackBuilder`, `IndexBuilder`, `AssetWatcher`, `AssetSource` |
| Rendering | `Mesh`, the `renderer` sub-module |
| Editor UI | the `editor_ui` and `tool_ui` modules (used by the editor and hub) |

The exact list is in
[`khora_sdk`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/index.html).

## Where things live

| You want to… | Reach for |
|---|---|
| Spawn an entity with a primitive shape | `Vessel::at(...)` + `spawn_*` helpers |
| Read or mutate a component | `world.get_component::<T>` / `world.get_component_mut::<T>` |
| Run a query | `world.query::<...>()` / `world.query_mut::<...>()` |
| Load an asset | `runtime.services.get::<Arc<AssetService>>()` |
| Save or load a scene | `runtime.services.get::<Arc<SerializationService>>()` |
| Read GPU or memory metrics | `runtime.services.get::<Arc<TelemetryService>>()` |
| Switch backends | Edit your `run_winit` bootstrap closure (`runtime.backends.insert(...)`) |
| Add a custom agent | Implement `Agent`, register in `AgentProvider::register_agents` |
| Add a custom phase | Return it from `PhaseProvider::custom_phases` |

---

*Concepts behind the surface: [Agents and Lanes](../concepts/agents-and-lanes.md),
[CLAD](../concepts/clad.md). Tasks: [How-to guides](../how-to/index.md). Every
signature: [rustdoc](https://eraflo.github.io/KhoraEngine/api/khora_sdk/index.html).*
