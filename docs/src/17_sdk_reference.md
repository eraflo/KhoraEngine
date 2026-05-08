# SDK reference

The full SDK surface. Every public type, organized by what you do with it.

- Document — Khora SDK Reference v1.0
- Status — Authoritative
- Date — May 2026

---

## Contents

1. The three application traits
2. `run_winit` — the entry point
3. `WindowConfig` and `WindowProvider`
4. `GameWorld` — the ECS facade
5. `Vessel` and the spawn helpers
6. `ServiceRegistry`
7. The prelude
8. Input
9. Engine modes
10. SDK re-exports
11. Where things live

---

## 01 — The three application traits

A Khora application implements three traits. The composite bound is `EngineApp + AgentProvider + PhaseProvider`.

### `EngineApp` — lifecycle

```rust
pub trait EngineApp: AgentProvider + PhaseProvider + Send + Sync {
    fn window_config() -> WindowConfig;
    fn new() -> Self;
    fn setup(&mut self, world: &mut GameWorld, services: &ServiceRegistry);
    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]);
    fn on_shutdown(&mut self) {}

    // Optional per-frame hooks (used by the editor for UI overlay)
    fn intercept_window_event(&mut self, event: &dyn Any, window: &dyn KhoraWindow) -> bool { false }
    fn before_frame(&mut self, world: &mut GameWorld, services: &ServiceRegistry, window: &dyn KhoraWindow) {}
    fn before_agents(&mut self, world: &mut GameWorld, services: &ServiceRegistry) {}
    fn after_agents(&mut self, world: &mut GameWorld, services: &ServiceRegistry) {}
}
```

| Method | When | What you do |
|---|---|---|
| `window_config()` | Once, before window creation | Return a `WindowConfig` |
| `new()` | Once, after window creation | Construct the struct — no engine context yet |
| `setup(world, services)` | Once, after engine init | Spawn entities; cache service handles |
| `update(world, inputs)` | Every frame | Game logic |
| `on_shutdown()` | Once, on exit | Cleanup |

The optional hooks (`intercept_window_event`, `before_frame`, `before_agents`, `after_agents`) exist so the editor can run an egui overlay around the engine's frame loop. Most games leave them at the default no-ops.

### `AgentProvider` — register custom agents

```rust
pub trait AgentProvider {
    fn register_agents(&self, dcc: &DccService, services: &mut ServiceRegistry);
}
```

The engine calls this once during boot. For a vanilla game with no custom subsystems, the body is empty. For an engine with a custom AI agent, scripting agent, networking agent, this is where you call `dcc.register_agent(...)` or `dcc.register_agent_for_mode(...)`.

### `PhaseProvider` — custom execution phases

```rust
pub trait PhaseProvider {
    fn custom_phases(&self) -> Vec<ExecutionPhase> { Vec::new() }
    fn removed_phases(&self) -> Vec<ExecutionPhase> { Vec::new() }
}
```

The built-in phases (defined in `khora-core::agent::ExecutionPhase`) are `INIT`, `OBSERVE`, `TRANSFORM`, `MUTATE`, `OUTPUT`, `FINALIZE` — IDs 0..=5. The default execution order is `INIT → OBSERVE → TRANSFORM → MUTATE → OUTPUT → FINALIZE`. Apps can insert custom phases (IDs 6..=254 via `ExecutionPhase::custom(id)`) — by default `Engine` inserts every custom phase **after** `OUTPUT`. Most games return empty vectors.

## 02 — `run_winit` — the entry point

```rust
pub fn run_winit<W: WindowProvider, A: EngineApp>(
    bootstrap: impl FnOnce(&dyn KhoraWindow, &mut ServiceRegistry, &dyn Any),
) -> anyhow::Result<()>;
```

`run_winit` opens a window through the provided `WindowProvider`, initializes the DCC, registers default services, runs your `bootstrap` closure, then enters the frame loop. It returns when the window closes or `on_shutdown` exits.

The bootstrap closure receives:

- `&dyn KhoraWindow` — the platform window (use to initialize the renderer).
- `&mut ServiceRegistry` — register your renderer and any custom services here.
- `&dyn Any` — opaque handle to the native event loop (downcast if you need it).

The standard bootstrap registers `WgpuRenderSystem`:

```rust
run_winit::<WinitWindowProvider, MyGame>(|window, services, _event_loop| {
    let mut rs = WgpuRenderSystem::new();
    rs.init(window).expect("renderer init failed");
    services.insert(rs.graphics_device());
    let rs: Box<dyn RenderSystem> = Box::new(rs);
    services.insert(Arc::new(Mutex::new(rs)));
})?;
```

`EngineCore` is the underlying engine type, exposed in case you need to construct an engine without `run_winit` (uncommon — only for embedding inside another runtime).

## 03 — `WindowConfig` and `WindowProvider`

### `WindowConfig`

```rust
pub struct WindowConfig {
    pub title: String,
    pub width: u32,            // default 1024
    pub height: u32,           // default 768
    pub icon: Option<WindowIcon>,
}
```

`WindowIcon` carries an RGBA8 pixel buffer plus dimensions for the platform window icon.

### `WindowProvider`

```rust
pub trait WindowProvider: 'static {
    fn create(native_loop: &dyn Any, config: &WindowConfig) -> Self where Self: Sized;
    fn request_redraw(&self);
    fn inner_size(&self) -> (u32, u32);
    fn scale_factor(&self) -> f64;
    fn as_khora_window(&self) -> &dyn KhoraWindow;
    fn translate_event(&self, raw_event: &dyn Any) -> Option<InputEvent>;
    fn clone_raw_window_arc(&self) -> Arc<dyn Any + Send + Sync>;
}
```

The default implementation is `WinitWindowProvider`. Alternative providers (SDL, custom embedded windowing) implement the same trait — `run_winit` is generic over it.

### `PRIMARY_VIEWPORT`

```rust
pub const PRIMARY_VIEWPORT: ViewportTextureHandle = ViewportTextureHandle(0);
```

Well-known handle for the primary 3D viewport. Use it when you need to refer to "the main rendering target" — for example, when the editor needs to render gizmos over the same view.

## 04 — `GameWorld` — the ECS facade

`GameWorld` is the safe entry point for the ECS. Internal types from `khora-data` are wrapped behind a stable surface.

### Lifecycle

| Method | Purpose |
|---|---|
| `GameWorld::new()` | Empty world |
| `GameWorld::from_world(world)` | Wrap an existing `World` (used for play mode restore) |
| `tick_maintenance()` | Run one ECS GC pass (called by the engine each frame) |

### Entities

```rust
let entity: EntityId = world.spawn((Transform::identity(), GlobalTransform::identity()));
let removed: bool = world.despawn(entity);
let entity = world.spawn_camera(camera);                 // Camera + GlobalTransform
let entity = world.spawn_entity(&transform);             // Transform + GlobalTransform
for id in world.iter_entities() { /* ... */ }
```

### Components

```rust
world.add_component(entity, my_component);
world.remove_component::<MyComponent>(entity);

let r: Option<&MyComponent>     = world.get_component::<MyComponent>(entity);
let m: Option<&mut MyComponent> = world.get_component_mut::<MyComponent>(entity);

// Convenience for Transform (the most-used component)
let r: Option<&Transform>     = world.get_transform(entity);
let m: Option<&mut Transform> = world.get_transform_mut(entity);
```

### Queries

```rust
for (t, g) in world.query::<(&Transform, &GlobalTransform)>() { /* read */ }
for (t,)   in world.query_mut::<(&mut Transform,)>()           { /* write */ }
```

### Transform synchronization

After mutating a `Transform`, the renderer reads `GlobalTransform`. Sync explicitly:

```rust
world.sync_global_transform(entity);

// Or do mutate + sync in one call:
world.update_transform(entity, |t| {
    t.translation += Vec3::Y;
});
```

### Assets

```rust
let mesh_handle: HandleComponent<Mesh> = world.add_mesh(my_mesh);
let mat_handle: MaterialComponent      = world.add_material(my_material);
```

### Internal access

`inner_world()` and `inner_world_mut()` expose the underlying `World` for low-level operations (serialization, tooling). Use sparingly — the wrapped surface is the supported API.

## 05 — `Vessel` and the spawn helpers

`Vessel` is a builder over a freshly spawned entity. Every `Vessel` has a `Transform` and a `GlobalTransform` from the start.

### Construction

```rust
Vessel::new(world)                       // at the origin
Vessel::at(world, position)              // at a specific position
```

### Builder methods

| Method | Effect |
|---|---|
| `with_transform(t)` | Replace the local `Transform` |
| `at_position(p)` | Replace just the translation |
| `with_rotation(q)` | Replace just the rotation |
| `with_scale(s)` | Replace just the scale |
| `with_component(c)` | Attach any `Component` (chainable) |
| `entity()` | Read the `EntityId` mid-build |
| `build()` | Finalize, sync `GlobalTransform`, return `EntityId` |

### Primitive helpers (top-level functions)

```rust
spawn_plane(world, size, y) -> Vessel
spawn_cube_at(world, position, size) -> Vessel
spawn_sphere(world, radius, segments, rings) -> Vessel
```

Each returns a `Vessel` you keep building on with `.with_component(...)` and finalize with `.build()`.

```rust
spawn_sphere(world, 0.75, 32, 16)
    .at_position(Vec3::new(0.0, 0.5, -5.0))
    .with_component(my_material)
    .build();
```

For non-primitive meshes, load through `AssetService` (see [Assets and VFS](./12_assets.md)) and attach the resulting `HandleComponent<Mesh>` via `.with_component(...)`.

## 06 — `ServiceRegistry`

The `ServiceRegistry` (re-exported from `khora-core`) is a typed container for services.

```rust
// Inside setup or update — services are passed in
fn setup(&mut self, world: &mut GameWorld, services: &ServiceRegistry) {
    let asset_service = services.get::<Arc<AssetService>>().unwrap();
    let mesh = asset_service.load_blocking("models/character.gltf").unwrap();
    /* ... */
}

// Inside the bootstrap closure — services are mutable, register your own
run_winit::<WinitWindowProvider, MyGame>(|window, services, _event_loop| {
    let custom = MyCustomService::new();
    services.insert(Arc::new(custom));
    /* ... */
})?;
```

### Engine-registered services
The engine inserts these into the registry during `EngineCore::initialize`, before your `setup` runs:

| Service | Crate | Purpose |
|---|---|---|
| `Arc<DccService>` | khora-control | DCC orchestration (cold-path thread, GORNA arbitration) |
| `Arc<TelemetryService>` | khora-telemetry | Metrics and monitor registry |
| `GpuCache` | khora-data | Shared GPU mesh store — handles to uploaded meshes |
| `ProjectionRegistry` | khora-data | Per-frame projection / mesh sync (runs `sync_all` before agents) |
| `SharedFrameGraph` | khora-data | `Arc<Mutex<FrameGraph>>` — per-frame pass collector, drained at `end_render_frame` |
| `RenderWorldStore` | khora-data | `Arc<RwLock<RenderWorld>>` populated each frame by `extract_scene` |
| `UiSceneStore` | khora-data | `Arc<RwLock<UiScene>>` populated each frame by `extract_ui_scene` |
| `PhysicsQueryService` | khora-agents | Raycasts and shape queries (registered only if a `PhysicsProvider` is present) |

### Bootstrap-registered services
Your `run_winit` closure registers the renderer and any custom services:

| Service | Crate | Purpose |
|---|---|---|
| `Arc<dyn GraphicsDevice>` | khora-core (trait) | The GPU device — read by `RenderAgent` directly |
| `Arc<Mutex<Box<dyn RenderSystem>>>` | khora-core (trait) | The render system — used at `begin_frame` / `end_frame` |
| `WgpuRenderSystem` | khora-infra | Default backend implementation |

### Frame-scoped services
Inserted into the per-frame service overlay (created fresh each tick):

| Service | Crate | Purpose |
|---|---|---|
| `FrameContext` | khora-core | Per-frame blackboard (color/depth targets, stages, async tasks) |
| `PRIMARY_VIEWPORT` | khora-sdk | Well-known viewport handle constant |

### On-demand services available through the SDK
Loaded once and served forever:

| Service | Crate | Purpose |
|---|---|---|
| `Arc<AssetService>` | khora-io | Asset loading through the VFS |
| `Arc<SerializationService>` | khora-io | Save and load scenes |
| `GpuMonitor`, `MemoryMonitor` | khora-infra | Hardware monitors feeding the DCC |

## 07 — The prelude

```rust
use khora_sdk::prelude::*;            // Common SDK types
use khora_sdk::prelude::ecs::*;       // ECS components
use khora_sdk::prelude::math::*;      // Math types
use khora_sdk::prelude::materials::*; // Materials
```

| Module | Contents |
|---|---|
| `prelude` | `WindowConfig`, `WindowIcon`, `PRIMARY_VIEWPORT`, `AssetHandle`, `AssetUUID`, `SaaTrackingAllocator`, `InputEvent`, `MouseButton` |
| `prelude::ecs` | `EntityId`, `Transform`, `GlobalTransform`, `Camera`, `Light`, `LightType`, `MaterialComponent`, `RigidBody`, `Collider`, `BodyType`, `ColliderShape`, `AudioSource`, `Parent`, `Children`, `Name`, `Without`, `Component`, `ComponentBundle`, `ProjectionType`, plus light variants |
| `prelude::materials` | `StandardMaterial`, `UnlitMaterial`, `EmissiveMaterial`, `WireframeMaterial` |
| `prelude::math` | `Vec2`, `Vec3`, `Vec4`, `Mat3`, `Mat4`, `Quaternion`, `Aabb`, `LinearRgba`, plus utilities |

The prelude is curated. Adding to it is a deliberate decision; we optimize for the import line being short and the imported names being unambiguous in context.

## 08 — Input

Inputs arrive in `update` as a `&[InputEvent]`. The variants:

```rust
pub enum InputEvent {
    KeyPressed { key_code: String },
    KeyReleased { key_code: String },
    MouseButtonPressed { button: MouseButton },
    MouseButtonReleased { button: MouseButton },
    MouseMoved { x: f32, y: f32 },
    MouseScrolled { delta: f32 },
    // ...
}
```

`key_code` strings follow the [W3C UI Events](https://www.w3.org/TR/uievents-code/) names — `"KeyW"`, `"Space"`, `"ShiftLeft"`, `"Escape"`. `MouseButton` covers `Left`, `Right`, `Middle`.

```rust
fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
    for ev in inputs {
        match ev {
            InputEvent::KeyPressed { key_code } if key_code == "Escape" => std::process::exit(0),
            InputEvent::MouseMoved { x, y } => self.look(*x, *y),
            _ => {}
        }
    }
}
```

Gamepad and touch are roadmap items.

## 09 — Engine modes

```rust
pub enum EngineMode {
    /// Simulation — scene cameras, physics, audio, ECS snapshot.
    Playing,
    /// A custom mode injected by a plugin (e.g., `Custom("editor")`).
    Custom(String),
}
```

The base engine knows only `Playing`. Other modes are injected by plugins. The editor application registers `EngineMode::Custom("editor")` and an editor-only `UiAgent` that lists `"editor"` in its `allowed_modes`. The Scheduler filters agents by the active mode each frame.

`EngineMode` controls **which agents run**. It is distinct from `PlayMode` (re-exported from `khora_core::ui::editor`), which is the editor's own UI-state enum:

```rust
pub enum PlayMode { Editing, Playing, Paused }
```

- `EngineMode` lives in `khora-core::agent::mode` and gates agent execution.
- `PlayMode` lives in `khora-core::ui::editor::state` and drives the editor's UI (Play / Stop / Pause buttons, panel visibility).

When the editor enters play mode, its `PlayMode` becomes `Playing` and it requests `EngineMode::Playing` from the engine; on stop, the editor restores its `Custom("editor")` mode and the world is snapshot-restored — see [Serialization](./14_serialization.md).

## 10 — SDK re-exports

The SDK is the single entry point. It re-exports types from internal crates so games never depend on them directly:

| Re-export | From | Purpose |
|---|---|---|
| `EngineCore`, `GameWorld` | khora-sdk | Engine + ECS facade |
| `Vessel`, `spawn_*` | khora-sdk | Spawn helpers |
| `EngineApp`, `AgentProvider`, `PhaseProvider`, `WindowProvider` | khora-sdk | App traits |
| `run_winit`, `WinitAppRunner`, `WinitWindowProvider` | khora-sdk | Bootstrap |
| `WindowConfig`, `WindowIcon`, `PRIMARY_VIEWPORT` | khora-sdk | Window types |
| `DccService`, `EngineMode`, `EngineContext`, `ExecutionScheduler`, `AgentRegistry` | khora-control | Control plane |
| `ExecutionPhase`, `AgentId`, `StrategyId`, `ServiceRegistry` | khora-core | Core types |
| `TelemetryService`, `TelemetryEvent`, `MonitoredResourceType` | khora-telemetry | Telemetry |
| `GpuMonitor`, `MemoryMonitor` | khora-infra | Hardware monitors |
| `WgpuRenderSystem` | khora-infra | Default render backend |
| `RenderSystem` | khora-core | The render trait |
| `SerializationService`, `SceneFile`, `SerializationGoal` | khora-io / khora-core | Scene I/O |
| `Mesh` | khora-core | Mesh type |
| `EditorState`, `UiTheme`, `PlayMode`, `GizmoMode`, `ViewportTextureHandle`, etc. | khora-core | Editor UI types (used by the editor) |

The `khora_sdk::editor_ui` module is a convenience namespace for the editor UI types. The `khora_sdk::renderer` module re-exports the renderer API submodules used by editor gizmos.

## 11 — Where things live

| You want to... | Reach for |
|---|---|
| Spawn an entity with a primitive shape | `Vessel::at(...)` + `spawn_*` helpers |
| Read or mutate a component | `world.get_component<T>` / `world.get_component_mut<T>` |
| Run a query | `world.query::<...>()` / `world.query_mut::<...>()` |
| Load an asset | `services.get::<Arc<AssetService>>()` |
| Save or load a scene | `services.get::<Arc<SerializationService>>()` |
| Read GPU or memory metrics | `services.get::<Arc<TelemetryService>>()` |
| Cast a ray | `services.get::<Arc<PhysicsQueryService>>()` |
| Switch backends | Edit your `run_winit` closure |
| Add a custom agent | Implement `Agent`, register in `AgentProvider::register_agents` |
| Add a custom phase | Return it from `PhaseProvider::custom_phases` |

For deeper internals (writing your own agent, lane, or backend), see [Extending Khora](./19_extending.md).

---

*Next: the editor. See [Editor](./18_editor.md).*
