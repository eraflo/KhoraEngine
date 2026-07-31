# Khora SDK — Guide

Concrete, code-grounded usage of the public `khora-sdk` API. The reference game is
`examples/sandbox/src/main.rs`.

- Document — Khora SDK Guide v1.0
- Status — Active

---

## 1 — A minimal game

```rust
use khora_sdk::prelude::*;
use khora_sdk::{run_winit, EngineApp, AgentProvider, PhaseProvider, GameWorld, Runtime, WindowConfig};
use khora_sdk::winit_adapters::WinitWindowProvider;

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

struct MyGame { frame: u64 }

impl EngineApp for MyGame {
    fn window_config() -> WindowConfig {
        WindowConfig { title: "My Game".into(), ..WindowConfig::default() }
    }
    fn new() -> Self { Self { frame: 0 } }
    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) { /* spawn entities */ }
    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) { self.frame += 1; }
}
impl AgentProvider for MyGame {
    fn register_agents(&self, _dcc: &khora_sdk::DccService, _rt: &mut Runtime) {}
}
impl PhaseProvider for MyGame {
    fn custom_phases(&self) -> Vec<khora_sdk::ExecutionPhase> { Vec::new() }
    fn removed_phases(&self) -> Vec<khora_sdk::ExecutionPhase> { Vec::new() }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    run_winit::<WinitWindowProvider, MyGame>(|window, runtime, _event_loop| {
        // Register backends here (see §4).
    })?;
    Ok(())
}
```

## 2 — The app lifecycle

- `window_config()` → your `WindowConfig` (title, size, icon).
- `new()` → simple constructor, **no engine context yet**.
- `setup(world, runtime)` → cache services from `runtime.resources`, spawn your initial entities.
- `update(world, inputs)` → your per-frame game logic. The engine renders/simulates around you.

`AgentProvider`/`PhaseProvider` are usually empty for a game — they exist for advanced extension.

## 3 — Spawning entities

Use the `Vessel` builder or the spawn helpers; never poke storage directly.

```rust
// A camera entity:
let cam = Camera::new_perspective(std::f32::consts::FRAC_PI_4, 16.0/9.0, 0.1, 1000.0);
let player = Vessel::at(world, Vec3::new(0.0, 2.0, 10.0))
    .with_component(cam)
    .with_rotation(Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::PI))
    .build();

// Primitives:
spawn_plane(world, 20.0, 0.0).build();
let sphere = spawn_sphere(world, 0.75, 32, 16).at_position(Vec3::new(0.0, 0.5, -5.0)).build();
let cube = spawn_cube_at(world, Vec3::new(2.0, 0.5, -3.0)).build();

// Materials:
let mat = world.add_material(StandardMaterial { base_color: LinearRgba::RED, roughness: 0.2, ..Default::default() });
spawn_sphere(world, 0.75, 32, 16).with_component(mat).build();

// Lights:
let mut sun = Light::directional();
if let LightType::Directional(ref mut d) = sun.light_type { d.intensity = 2.5; d.shadow_enabled = true; }
Vessel::at(world, Vec3::new(0.0, 20.0, 5.0)).with_component(sun).build();
```

`GameWorld` helpers: `spawn((components…))`, `get_transform_mut(entity)`, `sync_global_transform(entity)`,
`add_material(mat)`, `iter_entities()`, `set_parent(child, Some(parent))`.

## 4 — Registering backends (in the `run_winit` closure)

```rust
let mut rs = WgpuRenderSystem::new();
rs.init(window).expect("renderer init failed");
runtime.backends.insert(rs.graphics_device());       // before boxing — RenderAgent needs it
runtime.backends.insert(Arc::new(Mutex::new(Box::new(rs) as Box<dyn RenderSystem>)));

runtime.backends.insert(Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default()) as Box<dyn PhysicsProvider>)));
runtime.backends.insert(Arc::new(Mutex::new(Box::new(TaffyLayoutSystem::new()) as Box<dyn LayoutSystem>)));
runtime.backends.insert(Arc::new(StandardTextRenderer::new(TEXT_WGSL.to_owned())) as Arc<dyn TextRenderer>);

let mix_bus: Arc<dyn AudioMixBus> = Arc::new(DefaultMixBus::new(StreamInfo { channels: 2, sample_rate: 48_000 }, 8192));
runtime.resources.insert(Arc::clone(&mix_bus));
if let Ok(stream) = CpalAudioDevice::new().open(mix_bus) {
    runtime.backends.insert(Arc::<dyn AudioStream>::from(stream));
}
```

Register only the backends your game uses (a 2D-only game can skip physics/audio).

## 5 — Input

```rust
// In setup: bind actions on the engine InputMap (Arc<Mutex<InputMap>>) from runtime.resources.
if let Some(map) = runtime.resources.get::<Arc<Mutex<InputMap>>>() {
    let mut m = map.lock().unwrap();
    m.bind("player.forward", InputBinding::Key(KeyCode::KeyW));
    m.bind("player.forward", InputBinding::Key(KeyCode::ArrowUp)); // multiple bindings = OR
}
// In update: query held actions, and handle raw events for things the map can't model (mouse motion).
if input_map.is_pressed("player.forward") { /* move */ }
for ev in inputs { if let InputEvent::MouseMoved { x, y } = ev { /* look */ } }
```

## 6 — Materials, math, components

- Materials: `StandardMaterial` (PBR), `UnlitMaterial`, `EmissiveMaterial`, `WireframeMaterial`.
- Math: `Vec2/3/4`, `Quaternion`, `Mat4`, `LinearRgba` (with `RED`/`GREEN`/… constants). Y-up, right-handed.
- Components (`prelude::ecs`): `Transform`, `GlobalTransform`, `Camera`, `Light` (+ `LightType`,
  `DirectionalLight`/`PointLight`/`SpotLight`), `RigidBody` (+ `BodyType`), `Collider` (+ `ColliderShape`),
  `AudioSource`, `Name`, `Tag`, `Parent`/`Children`, `MaterialComponent`.

## 7 — Scenes & shipping

- Scenes: `SceneFile` + `SerializationGoal` (Editor Interchange / Fastest Load / Smallest File / …).
- Packing: `PackBuilder` produces an asset pack; `run_default` boots a packed runtime
  (`khora-runtime`). See the [`pack-and-ship`](./skills/pack-and-ship/SKILL.md) skill.
- The **editor** (`cargo run -p khora-editor` in the engine repo) authors scenes/prefabs visually.

---

*When in doubt, read `examples/sandbox/src/main.rs` — it exercises this whole surface.*
