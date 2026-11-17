# SDK surface

<p class="kp-lede">A game talks to the engine through one door, the SDK. Behind it are a dozen rooms, the engine's crates, but a game never walks into them: everything it needs is handed out at the door.</p>

A curated map of the public `khora-sdk` surface. `khora-sdk` is the only crate a
game depends on; it re-exports the types it needs from internal crates so games
never reference those directly.

> This page names the surface and links to rustdoc for the exhaustive signatures.
> For the full method-by-method detail of any type below, follow its link to the
> [published rustdoc](https://eraflo.github.io/KhoraEngine/api/khora_sdk/index.html).
> For *how* to build a game, see [Your first game](../tutorials/your-first-game.md)
> and the [How-to guides](../how-to/index.md).

## The application traits

A Khora application implements two traits: `EngineApp`, which requires
`PhaseProvider`. A third, `WindowProvider`, abstracts the window backend and comes
with a default.

| Trait | Role | rustdoc |
|---|---|---|
| [`EngineApp`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.EngineApp.html) | Application lifecycle — `window_config`, `game_identity`, `keeps_user_files`, `new`, `setup`, `provide`, `update`, `startup_ready`, `on_shutdown`, plus optional overlay hooks. | link |
| [`PhaseProvider`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.PhaseProvider.html) | Insert or remove custom `ExecutionPhase`s. | link |
| [`WindowProvider`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.WindowProvider.html) | Creates the platform window `run_winit` drives (default: `WinitWindowProvider`). | link |

### `EngineApp` — lifecycle

The methods the engine calls on your app type, in the order of a run:

| Method | When |
|---|---|
| `window_config() -> WindowConfig` | Before the window is created (hidden until the first frame is painted). The default `game_identity` reads it too. |
| `game_identity() -> GameIdentity` | At boot: the game the engine files what it learns under, kept as `Arc<GameIdentity>`. Default: the window's title at `ENGINE_VERSION`. |
| `keeps_user_files() -> bool` | At boot: whether the engine keeps its files (this machine's profile of the game, the quarantine) in the user's folders across runs. Default `false`: no user folder is touched, unless `KHORA_PROFILES` is set. |
| `new() -> Self` | Once, after the window is created and before the bootstrap closure — no engine context yet. |
| `setup(&mut self, world: &mut GameWorld, runtime: &Runtime)` | Once, after engine init — spawn entities, cache handles. |
| `provide(&self, runtime: &mut Runtime)` | Once at boot, after `setup` and before the engine's families are built — install the app's own resources or services (default: nothing). |
| `update(&mut self, world: &mut GameWorld, inputs: &[InputEvent])` | Every frame — game logic. |
| `startup_ready(&mut self)` | Once, at the boundary of the first frame that finds startup over: every ticket waited on through `Readiness` landed and no hold is kept, or the wait timed out (default 60 s; the late ones are reported to `Problems`). The place to drop a loading screen. Default: nothing. |
| `on_shutdown(&mut self)` | Once, on exit (default no-op). |

The optional hooks `intercept_window_event`, `before_frame`, `before_agents`, and
`after_agents` exist so a tool can run a UI overlay around the engine's frame
loop, as the editor does with egui. Most games leave them at their default
no-ops. `before_frame` runs before the engine fixes the frame: a family switched
on or off there (see [Activation](#activation--which-families-run)) is in force
for that whole frame.

`setup` and the optional hooks receive `&Runtime` — the engine's injection bundle
(see [Runtime](#runtime--the-injection-bundle) below).

### `provide` — install what the app needs

```rust
fn provide(&self, runtime: &mut Runtime) {}
```

Called once at boot, after `setup` and before the engine's families are built.
What it inserts into the runtime is there when every family boots. It never
registers an agent: a new subsystem is a **family** — its lanes, a `LaneFamily`,
and one `register_agent!` registration — and every family's agent is the engine's
one `LaneAgent`. See [Add a subsystem (a family)](../how-to/add-a-family.md).

### `PhaseProvider` — custom execution phases

```rust
fn custom_phases(&self) -> Vec<ExecutionPhase> { Vec::new() }
fn removed_phases(&self) -> Vec<ExecutionPhase> { Vec::new() }
```

The built-in phases live in `khora_core::agent::ExecutionPhase` and run `INIT`,
`OBSERVE`, `TRANSFORM`, `MUTATE`, `OUTPUT`, `FINALIZE`. Added phases run after
`OUTPUT` and before `FINALIZE`, in the order declared. Removing the only phase a
family runs in refuses the boot. Most games return empty vectors. See the
[`ExecutionPhase`](./glossary.md#executionphase--tickphase) glossary entry.

## `run_winit` — the entry point

```rust
pub fn run_winit<W: WindowProvider, A: EngineApp>(
    bootstrap: impl FnOnce(&dyn KhoraWindow, &mut Runtime, &dyn Any) + 'static,
) -> anyhow::Result<()>;
```

Drives a winit event loop. It creates the window through `W`, hidden; calls
`A::new()`; runs your `bootstrap` closure; then boots the engine — the seal
check below, its services, the DCC, `setup`, `provide`, the families — paints
the first frame, shows the window, and enters the frame loop. It returns when
the window closes. The bootstrap closure receives:

- `&dyn KhoraWindow` — the platform window (use it to initialize the renderer).
- `&mut Runtime` — register your renderer and any custom backends/services here.
- `&dyn Any` — the winit `ActiveEventLoop` (downcast it if needed; `khora_sdk::winit`
  is re-exported for that).

`WinitWindowProvider` is the default `WindowProvider`. `EngineCore` is the
underlying engine type, and `EngineBootstrap` boots it in stages (`begin`,
`setup`, `register`, `finish`) for embedding `khora-sdk` without `run_winit`
(uncommon).

`run_default() -> anyhow::Result<()>` is the zero-config entry the prebuilt
`khora-runtime` binary uses. It reads `runtime.json` beside the binary, checks
the seal before any window opens (an error rather than a panic), wires the
default backends (wgpu, Rapier, Taffy, CPAL and its mix bus), opens the game's
assets, and loads the scene `runtime.json` names, else `scenes/default.kscene`
(`DEFAULT_SCENE_REL_PATH`).

### The seal at every start

A built game is a binary stamped with a key, beside its signed pack
(`data.pack`, `index.bin`, `manifest.bin`). Every engine start, whatever the
entry point, checks the binary it runs from against what lies beside it, before
anything else of the boot:

| Beside the binary | Binary with no key (a dev run, a tool, a test) | Stamped binary (a built game) |
|---|---|---|
| no pack | starts | refused: a built game runs from its pack |
| a pack | refused (`PackSealError::Unstamped`) | starts only if `manifest.bin` verifies with the key over `index.bin` |

A refusal stops the start: `run_winit` and `EngineCore` panic, naming the folder
and why; `run_default` returns the error before a window opens. See
[Trust: signed packs](../concepts/trust.md#signed-packs).

### `open_game_assets` — the game's assets

```rust
pub fn open_game_assets<A: EngineApp>(runtime: &mut Runtime, exe_dir: &Path) -> anyhow::Result<()>;
```

Called from the bootstrap closure, it opens the assets of the game `A` beside
`exe_dir` into the runtime:

- a packed game: the pack, with every check (its index read under its size cap,
  its seal verified, the pack and its index counting the same entries), the
  player's profile folder, and a script compiler that reads the pack only;
- a binary with no key and no pack: loose `assets/`;
- the run's refusals, installed first, so every reader reports to them;
- `Arc<Mutex<AssetService>>` in `runtime.services`.

It fails when the seal refuses the pack, when the index cannot be read or
disagrees with the pack, or when there is neither a pack nor `assets/`. The
boot then files what it opened, off the frame: the pack's scripts to compile, or
the loose files to index and their scripts to compile, the startup waiting on
each through `Readiness`. `run_default` calls it for you.

## `Runtime` — the injection bundle

`Runtime` (re-exported from `khora-core`) is the engine-wide injection point.
Engine init builds one, wraps it in `Arc<Runtime>`, and from then on it is
immutable. It bundles three typed containers, each with a clear admission rule:

| Container | Holds | Examples |
|---|---|---|
| `runtime.services` | Concrete stateful objects with a rich business API. | `Arc<Mutex<AssetService>>` (inserted by `run_default`, or by your bootstrap), `Arc<Mutex<ScriptRuntime>>` (inserted by the engine) |
| `runtime.backends` | Concrete impls of abstract `khora-core` traits. | `dyn RenderSystem`, `dyn PhysicsProvider`, `dyn AudioDevice`, `dyn LayoutSystem` |
| `runtime.resources` | Long-lived shared state without a service-style API. | `AssetStore`, `InputMap`, viewport overrides |

Look up an entry with `runtime.services.get::<T>()`, `runtime.backends.get::<T>()`,
or `runtime.resources.get::<T>()` (each returns `Option<&T>`; `require::<T>()`
panics if absent). Per-frame state (current viewport, frame deltas, lane outputs)
does **not** live here — it flows through the `LaneBus` and `OutputDeck`.

Not everything with a service-style API is in `runtime.services`. The
`DccService` and the `TelemetryService` are owned by `EngineCore`. The `SerializationService` is not
registered at all: it holds nothing but where prefabs are read from, so it is
constructed where it is needed (see [Saving and loading](#saving-and-loading)).

Among what the engine installs at boot: `Arc<Activation>` (below),
`Arc<Readiness>`, the startup's wait (`wait_for_index`, `wait_for_scene`,
`wait_for_compile`, `hold`), and `Arc<SceneLoadHub>`, where a running game files
a scene load to be prepared off the frame and reads its outcome by ticket.

## `GameWorld` — the ECS facade

`GameWorld` is the safe public entry point for the ECS; it wraps the internal
`khora-data` `World`. The surface, grouped by what you do:

| Group | Methods |
|---|---|
| Lifecycle | `new`, `from_world`, `set_prefabs`, `set_scene_loads`, `scene_loads` |
| Entities | `spawn`, `despawn`, `spawn_camera`, `spawn_entity`, `iter_entities` |
| Components | `add_component`, `remove_component`, `get_component`, `get_component_mut`, `get_transform`, `get_transform_mut` |
| Script components | `add_declared_component(entity, name, fields)` — attaches a component an Ergon script declares (a behavior, a `component`) by name; refused with `DeclaredComponentError` for a Rust component, a behavior's state or a system's component |
| Queries | `query::<...>()`, `query_mut::<...>()` |
| Transforms | `sync_global_transform`, `update_transform` |
| Hierarchy | `set_parent` |
| Assets | `add_mesh`, `add_material` |
| Game saves | `save_game`, `load_game`; `file_load_game` and `take_load_outcome` to load one off the frame |
| Internal | `inner_world`, `inner_world_mut` (low-level; prefer the wrapped surface) |

After mutating a `Transform`, call `sync_global_transform(entity)` (or use
`update_transform`, which mutates and syncs in one call) so the renderer sees the
updated `GlobalTransform`. It copies the entity's own `Transform` and ignores a
parent: a child's `GlobalTransform` is set right by transform propagation, every
frame. Full signatures:
[`GameWorld`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.GameWorld.html).

## Saving and loading

Two operations, two different documents — see
[Scenes and game saves](../concepts/saves.md) for why.

**A game save** is how a running world differs from the scene it started from.
`GameWorld` carries it:

```rust
// `level` is the scene file the game started from, known as `level_id`.
let save: SceneFile = world.save_game(level_id, &level, SerializationGoal::SmallestFileSize)?;
khora_sdk::fs::write_atomic(path, &save.to_bytes())?;

// Later: the scene as it is now, with the save's differences on top.
let save = SceneFile::from_bytes(&std::fs::read(path)?)?;
let report: LoadReport = world.load_game(&save, &level)?;
```

`load_game` replaces the world's contents atomically: if the save cannot be
loaded, the world is left exactly as it was. While frames run, prefer
`file_load_game(&save, &level)`, which returns a `LoadTicket`: the load is
prepared off the frame, lands at a later frame boundary, and
`take_load_outcome(ticket)` returns its `LoadReport` or error once it has. Where
a save file lives is the game's decision — the engine hands back bytes and names
no folder. Asking for `FastestLoad` writes a game save compactly, because a
snapshot cannot hold the runtime state a save exists to keep.

The engine calls `set_prefabs` on the world it hands to `setup` when an
`Arc<Mutex<AssetService>>` is in `runtime.services` at boot (put there by the
bootstrap closure or `open_game_assets`; one inserted in `provide` comes too
late), so a level holding [prefab instances](../concepts/prefabs.md) is compared
against the level expanded. A world you construct yourself starts without a
prefab source.

**A scene** is the whole world, written down. Construct a
`SerializationService` and pass it the inner `World`:

```rust
let service = SerializationService::new();
let file = service.save_world(world.inner_world(), SerializationGoal::HumanReadableDebug)?;
let report = service.replace_world(&file, world.inner_world_mut())?; // or load_world, beside what is there
```

`SerializationService::with_prefabs(source)` writes prefab instances as links
and expands them on load; without a source, instances are saved expanded and a
scene that links to prefabs cannot be loaded. Every load returns a `LoadReport`
listing what it adapted — a renamed field, a defaulted one, a retired component —
and fails with `SerializationServiceError` rather than leave a half-loaded world.
The `SerializationGoal` chooses the encoding, never the contents; see
[Serialization](../concepts/serialization.md).

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
| `prelude::ecs` | `EntityId`, `Transform`, `GlobalTransform`, `Camera`, `CameraProblem`, `Light`, `LightType`, `DirectionalLight`, `PointLight`, `SpotLight`, `MeshRef`, `MaterialRef`, `RigidBody`, `Collider`, `BodyType`, `ColliderShape`, `AudioSource`, `Parent`, `Children`, `Name`, `Tag`, `Without`, `Changed`, `Mut`, `Component`, `ComponentBundle`, `ProjectionType`, `ProceduralMeshKind`, `ScriptValue` |
| `prelude::materials` | `StandardMaterial`, `UnlitMaterial`, `EmissiveMaterial`, `WireframeMaterial`, `AlphaMode` |
| `prelude::math` | `LinearRgba` and everything in `khora_core::math` (`Vec2/3/4`, `Mat3/4`, `Quaternion`, `Aabb`, …) |

The prelude is curated — adding to it is a deliberate decision. Full contents:
[`prelude`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/prelude/index.html).

## Input

Inputs arrive in `update` as `&[InputEvent]`. `InputEvent` and `KeyCode` /
`MouseButton` are re-exported at the crate root and in the prelude. A key is a
`KeyCode` enum value whose variants are named after the
[W3C UI Events](https://www.w3.org/TR/uievents-code/) codes (`KeyCode::KeyW`,
`KeyCode::Space`, `KeyCode::Escape`). See [Map input](../how-to/map-input.md) and the
[`InputEvent`](https://eraflo.github.io/KhoraEngine/api/khora_core/platform/enum.InputEvent.html)
rustdoc for the full variant list.

## Activation — which families run

The engine has no mode. It runs **families**, every one registered with
`register_agent!` (rendering, shadows, physics, audio, UI, scripting, loading,
the overlays), and an app switches them on and off.
`khora_sdk::Activation` (from `khora_core::agent`) is published in the runtime
as `Arc<Activation>` before the app's `setup`. Every family starts on.

```rust
if let Some(activation) = runtime.resources.get::<Arc<Activation>>() {
    // A request: scripts stop at the next frame boundary.
    let _ = activation.deactivate(khora_core::script::SCRIPT);
}
```

`activate` and `deactivate` are requests, and so is
`scope(family, "Marker")`: the family runs on the entities holding the component
registered as `Marker` only, and counts as on (the boundary warns when no
component has that name). A family the activation does not know is refused with
`UnknownFamily`, and logged. The SDK settles the requests once
per frame, after `before_frame` and before the first DataSystem phase, so every
phase, Flow, the scheduler and the DCC see one state; a request made later
counts from the next frame. A family off is not asked to offer, is lent nothing,
and does not run. The DataSystems and Flows declared `Role::Serves(family)` are
skipped with it, and its `BetweenSessions::Drop` mailboxes are emptied at each
boundary. The family ids live beside their domain:
`khora_core::script::SCRIPT`, `khora_core::physics::PHYSICS`,
`khora_core::audio::AUDIO`; any family's is its `LaneFamily::AGENT`.

The editor switches scripting, physics and audio off while editing and on at
Play. That is the editor's choice, made from `PlayMode` (`Editing` / `Playing` /
`Paused`), its own UI-state enum re-exported from `khora_core::ui::editor`, which
the engine never reads. See
[The frame](../concepts/the-frame.md#the-simulation-clock).

## SDK re-exports

`khora-sdk` re-exports types from internal crates so games depend on the SDK
alone. The major groups:

| Group | Re-exported types (selection) |
|---|---|
| Engine + ECS | `EngineCore`, `EngineBootstrap`, `GameWorld`, `DeclaredComponentError`, `Vessel`, `spawn_*`, `HandleComponent` |
| App traits | `EngineApp`, `PhaseProvider`, `WindowProvider`, `GameIdentity`, `ENGINE_VERSION` |
| Bootstrap | `run_winit`, `run_default`, `WinitAppRunner`, `WinitWindowProvider`, `RuntimeConfig`, `DEFAULT_SCENE_REL_PATH` |
| Startup | `Readiness`, `ReadinessHold`, `ReadinessProgress`, `ReadinessState`, `ReadinessTicket` |
| Window | `WindowConfig`, `WindowIcon`, `PRIMARY_VIEWPORT` |
| Runtime / control | `Runtime`, `Services`, `Backends`, `Resources`, `DccService`, `DccConfig`, `Activation`, `AgentRegistry` |
| Core types | `ExecutionPhase`, `ExecutionTiming`, `AgentId`, `StrategyId`, `AgentImportance` |
| Telemetry | `TelemetryService`, `TelemetryEvent`, `MonitoredResourceType`, `MetricsRegistry`, `MonitorRegistry` |
| Monitors | `MemoryMonitor` |
| Backends + traits | `WgpuRenderSystem`, `RenderSystem`, `WgpuPipelineSystem`, `PipelineSystem`, `RapierPhysicsWorld`, `PhysicsProvider`, `CpalAudioDevice`, `AudioDevice`, `DefaultMixBus`, `AudioMixBus`, `TaffyLayoutSystem`, `LayoutSystem` |
| Scene I/O | `SerializationService`, `SerializationServiceError`, `SceneFile`, `SerializationGoal`, `LoadReport`, `SceneLoadHub`, `LoadOutcome`, `ComponentRegistration`, `serialize_subtree`, `instantiate_subtree` |
| Assets | `open_game_assets`, `AssetService`, `AssetIo`, `FileLoader`, `PackLoader`, `PackBuilder`, `IndexBuilder`, `IndexHub`, `AssetWatcher`, `AssetSource` |
| Signed packs | `PackKey`, `PackSigner`, `SignedManifest`, `PackSealError`, `stamp_pack_key`, `stamped_pack_key` |
| Trust | the `trust` module: `Trust`, `ProjectTrust`, refusals and the quarantine |
| Files | the `fs` module: `write_atomic` (a crash mid-write leaves the previous file whole), `read_bounded` (refuses a file past a size cap), paths under a root, walking a tree; use it rather than `std::fs` for a save |
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
| Load an asset | `runtime.services.get::<Arc<Mutex<AssetService>>>()` |
| Save or load a game | `world.save_game(...)` / `world.load_game(...)` |
| Save or load a scene | `SerializationService::new()` + `save_world` / `replace_world` |
| Choose which families run | `activate` / `deactivate` on the `Arc<Activation>` resource |
| Switch backends | Edit your `run_winit` bootstrap closure (`runtime.backends.insert(...)`) |
| Add a subsystem | A family: its lanes, a `LaneFamily` and one `register_agent!` registration — never a new agent type ([Add a subsystem (a family)](../how-to/add-a-family.md)) |
| Install the app's own resources | `EngineApp::provide` |
| Add a custom phase | Return it from `PhaseProvider::custom_phases` |

---

*Concepts behind the surface: [Agents and Lanes](../concepts/agents-and-lanes.md),
[CLAD](../concepts/clad.md). Tasks: [How-to guides](../how-to/index.md). Every
signature: [rustdoc](https://eraflo.github.io/KhoraEngine/api/khora_sdk/index.html).*
