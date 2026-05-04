# SDK quickstart

A working game in under a hundred lines, built from the actual sandbox example.

- Document — Khora SDK Quickstart v1.0
- Status — Tutorial
- Date — May 2026

---

## Contents

1. Prerequisites
2. The pieces you need
3. The minimum game
4. Walking through it
5. The bootstrap closure
6. Vessel — the spawn helper
7. Adding behavior
8. Where to go from here

---

## 01 — Prerequisites

- Rust 1.85+ (edition 2024).
- A GPU that supports Vulkan, Metal, or DX12.
- Git, for cloning the repo.

```bash
git clone https://github.com/eraflo/KhoraEngine
cd KhoraEngine
cargo build
cargo test --workspace
```

If `cargo test --workspace` passes, you have a working environment. The shipping demo is `cargo run -p sandbox`.

## 02 — The pieces you need

A Khora game has four moving parts:

| Piece | What it is |
|---|---|
| Your **app struct** | Holds game state. Implements `EngineApp`, `AgentProvider`, `PhaseProvider`. |
| **`run_winit`** | The bootstrap entry point. Generic over a window provider and your app. |
| **`WgpuRenderSystem`** | The default rendering backend. You construct it inside the bootstrap closure and register it as a service. |
| **The bootstrap closure** | Wires services (renderer, custom services) before the engine starts spinning. |

The pattern is *intentionally explicit*. There is no hidden global setup — you can see where every dependency comes from.

## 03 — The minimum game

Drop this into a fresh crate's `main.rs` (or read along with `examples/sandbox/src/main.rs`):

```rust
use anyhow::Result;
use khora_sdk::prelude::math::{Quaternion, Vec3};
use khora_sdk::prelude::*;
use khora_sdk::run_winit;
use khora_sdk::winit_adapters::WinitWindowProvider;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, GameWorld, InputEvent,
    PhaseProvider, RenderSystem, ServiceRegistry, WgpuRenderSystem,
    WindowConfig,
};
use std::sync::{Arc, Mutex};

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

struct MyGame;

impl EngineApp for MyGame {
    fn window_config() -> WindowConfig {
        WindowConfig {
            title: "My Khora Game".to_owned(),
            ..WindowConfig::default()
        }
    }

    fn new() -> Self {
        MyGame
    }

    fn setup(&mut self, world: &mut GameWorld, _services: &ServiceRegistry) {
        // A camera looking at the origin
        let camera = khora_sdk::prelude::ecs::Camera::new_perspective(
            std::f32::consts::FRAC_PI_4,
            16.0 / 9.0,
            0.1,
            1000.0,
        );
        khora_sdk::Vessel::at(world, Vec3::new(0.0, 2.0, 10.0))
            .with_component(camera)
            .with_rotation(Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::PI))
            .build();

        // A flat ground plane
        khora_sdk::spawn_plane(world, 20.0, 0.0).build();

        // A directional sun light with shadows
        let mut sun = khora_sdk::prelude::ecs::Light::directional();
        if let khora_sdk::prelude::ecs::LightType::Directional(ref mut d) = sun.light_type {
            d.intensity = 2.5;
            d.shadow_enabled = true;
        }
        khora_sdk::Vessel::at(world, Vec3::new(0.0, 20.0, 5.0))
            .with_component(sun)
            .with_rotation(Quaternion::from_axis_angle(
                Vec3::X,
                -std::f32::consts::FRAC_PI_2 * 0.8,
            ))
            .build();

        // A red sphere in front of the camera
        let mat = khora_sdk::prelude::materials::StandardMaterial {
            base_color: khora_sdk::prelude::math::LinearRgba::RED,
            roughness: 0.2,
            ..Default::default()
        };
        let mat_handle = world.add_material(mat);
        khora_sdk::spawn_sphere(world, 0.75, 32, 16)
            .at_position(Vec3::new(0.0, 0.5, -5.0))
            .with_component(mat_handle)
            .build();
    }

    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {
        // Game logic — empty for now
    }
}

impl AgentProvider for MyGame {
    fn register_agents(&self, _dcc: &DccService, _services: &mut ServiceRegistry) {
        // No custom agents in this example
    }
}

impl PhaseProvider for MyGame {
    fn custom_phases(&self) -> Vec<khora_sdk::ExecutionPhase> { Vec::new() }
    fn removed_phases(&self) -> Vec<khora_sdk::ExecutionPhase> { Vec::new() }
}

fn main() -> Result<()> {
    env_logger::init();

    run_winit::<WinitWindowProvider, MyGame>(|window, services, _event_loop| {
        let mut rs = WgpuRenderSystem::new();
        rs.init(window).expect("renderer init failed");
        // RenderAgent reads the graphics device directly — register it before
        // boxing the system.
        services.insert(rs.graphics_device());
        let rs: Box<dyn RenderSystem> = Box::new(rs);
        services.insert(Arc::new(Mutex::new(rs)));
    })?;
    Ok(())
}
```

Run it with `cargo run --release`. A window opens. You see a red sphere on a ground plane, lit by a sun. The frame rate appears in the editor's status bar (or whatever your terminal logs).

## 04 — Walking through it

### `MyGame` and the three traits

Every Khora app implements three traits:

| Trait | Purpose |
|---|---|
| `EngineApp` | Lifecycle: `window_config`, `new`, `setup`, `update`, `on_shutdown` (and a few optional hooks) |
| `AgentProvider` | Where you register custom agents with the DCC |
| `PhaseProvider` | Where you declare custom execution phases (or remove default ones) |

For a basic game, `AgentProvider` and `PhaseProvider` are no-ops. They become interesting when you write your own subsystems — see [Extending Khora](./19_extending.md).

### The `EngineApp` lifecycle

| Method | When | What you do |
|---|---|---|
| `window_config()` | Once, before window creation | Return a `WindowConfig` (title, size, icon) |
| `new()` | Once, after window creation | Construct the struct — no engine context yet |
| `setup(world, services)` | Once, after engine init | Spawn entities. `services` gives you renderer access if needed |
| `update(world, inputs)` | Every frame | Game logic — read `inputs`, mutate `world` |
| `on_shutdown()` | Once, on exit | Cleanup |
| `before_frame` / `before_agents` / `after_agents` | Optional per-frame hooks | Used by the editor for UI overlay; rarely needed in games |

### `setup` — spawning the world

`setup` runs once. You spawn cameras, lights, geometry, and any persistent state. The signature gives you a mutable `GameWorld` and an immutable `&ServiceRegistry`:

- `world.spawn(...)` for raw tuple bundles.
- `Vessel::at(world, position).with_component(c).build()` for the builder path.
- `spawn_plane`, `spawn_sphere`, `spawn_cube_at` for primitive helpers — all return a `Vessel` you keep building on.
- `world.add_material(m)` registers a material and returns a `MaterialComponent` you attach via `.with_component(...)`.

### `update` — the per-frame hook

`update` runs every frame. The `inputs` slice contains `InputEvent` values translated from the platform window — keys, mouse buttons, mouse moves, scrolls. You mutate the world to reflect game logic.

After mutating a `Transform`, call `world.sync_global_transform(entity)` to propagate to the renderer. (`update_transform` does the mutation and the sync in one call.)

### `#[global_allocator]`

The example installs `SaaTrackingAllocator`, the heap-tracking allocator that feeds the DCC's memory heuristics. It is optional — without it, memory metrics are absent. See [Telemetry](./15_telemetry.md).

## 05 — The bootstrap closure

```rust
run_winit::<WinitWindowProvider, MyGame>(|window, services, _event_loop| {
    let mut rs = WgpuRenderSystem::new();
    rs.init(window).expect("renderer init failed");
    services.insert(rs.graphics_device());
    let rs: Box<dyn RenderSystem> = Box::new(rs);
    services.insert(Arc::new(Mutex::new(rs)));
})?;
```

`run_winit` is the engine entry point. It is generic over:

- A `WindowProvider` (here, `WinitWindowProvider`, the default winit-based implementation).
- Your `EngineApp` type.

The closure is your one chance to wire **services** before the engine starts. The default services (DCC, telemetry, asset service) are registered by the engine itself; the renderer is registered by *you* because the engine is backend-agnostic. Two registrations are needed:

1. The graphics device (`Arc<dyn GraphicsDevice>`) — `RenderAgent` reads it directly.
2. The boxed render system (`Arc<Mutex<Box<dyn RenderSystem>>>`) — used by frame submission.

To swap rendering backends, change these two registrations. Lanes hold `Arc<dyn GraphicsDevice>` and never know which backend is underneath.

## 06 — Vessel — the spawn helper

`Vessel` is the SDK's spawn builder. It guarantees every entity has a `Transform` and a `GlobalTransform`, and lets you attach components fluently.

### Construction

```rust
// At the origin
let e = Vessel::new(world).build();

// At a position
let e = Vessel::at(world, Vec3::new(0.0, 2.0, 10.0)).build();
```

### Builder methods

```rust
Vessel::at(world, pos)
    .with_transform(custom_transform)        // override the local Transform
    .at_position(other_pos)                  // change just the translation
    .with_rotation(quat)                     // change just the rotation
    .with_scale(Vec3::new(2.0, 1.0, 1.0))    // change just the scale
    .with_component(my_camera)               // attach any Component
    .with_component(my_light)                // chain as many as needed
    .build()                                 // returns EntityId
```

### Primitive helpers

For prototyping, three top-level functions return a pre-loaded `Vessel`:

| Function | Mesh |
|---|---|
| `spawn_plane(world, size, y)` | XZ plane at height `y`, side length `size` |
| `spawn_cube_at(world, pos, size)` | Centered cube at `pos`, side length `size` |
| `spawn_sphere(world, radius, segments, rings)` | UV sphere at the origin |

Each returns a `Vessel` — chain `.at_position(...)`, `.with_component(...)`, `.build()` to finish.

```rust
spawn_sphere(world, 0.75, 32, 16)
    .at_position(Vec3::new(2.0, 0.5, -10.0))
    .with_component(my_material)
    .build();
```

For non-primitive meshes, load through `AssetService` (see [Assets and VFS](./12_assets.md)) and attach the resulting `HandleComponent<Mesh>` via `.with_component(...)`.

## 07 — Adding behavior

To make the sphere move when the player presses W:

```rust
struct MyGame {
    sphere: Option<EntityId>,
    forward: bool,
}

impl EngineApp for MyGame {
    fn new() -> Self {
        MyGame { sphere: None, forward: false }
    }

    fn setup(&mut self, world: &mut GameWorld, _services: &ServiceRegistry) {
        // ... camera, plane, light as before ...
        self.sphere = Some(
            spawn_sphere(world, 0.75, 32, 16)
                .at_position(Vec3::new(0.0, 0.5, -5.0))
                .build(),
        );
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        for ev in inputs {
            match ev {
                InputEvent::KeyPressed { key_code } if key_code == "KeyW" => self.forward = true,
                InputEvent::KeyReleased { key_code } if key_code == "KeyW" => self.forward = false,
                _ => {}
            }
        }

        if self.forward {
            if let Some(e) = self.sphere {
                world.update_transform(e, |t| {
                    t.translation += Vec3::new(0.0, 0.0, -0.05);
                });
            }
        }
    }
}
```

`update_transform` mutates *and* syncs `GlobalTransform` in one call — no need to call `sync_global_transform` separately.

For more substantial behavior — AI, scripting, networking — write a custom agent (see [Extending Khora](./19_extending.md)). The `update` method is for per-frame application logic, not for engine subsystems.

## 08 — Where to go from here

You have a running, interactive program. From here:

- **[SDK reference](./17_sdk_reference.md)** — full API surface: `EngineApp`, `GameWorld`, `Vessel`, `WindowConfig`, prelude.
- **[ECS — CRPECS](./05_ecs.md)** — how queries, archetypes, and component bundles work.
- **[Assets and VFS](./12_assets.md)** — loading meshes, textures, audio.
- **[Physics](./10_physics.md)** — adding `RigidBody` and `Collider` for simulation.
- **[Audio](./11_audio.md)** — spawning `AudioSource` for spatial audio.
- **[Editor](./18_editor.md)** — running `khora-editor` to author scenes visually.
- **[Extending Khora](./19_extending.md)** — writing custom agents and lanes.

The full sandbox lives at `examples/sandbox/src/main.rs`. It adds a free-fly camera controller, multiple lights, and a small entity grid. Read it once you are comfortable with the basics.

---

*Next: the full SDK surface. See [SDK reference](./17_sdk_reference.md).*
