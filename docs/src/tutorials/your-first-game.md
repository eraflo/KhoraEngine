# Your first game

By the end of this lesson you will have a running Khora window showing a lit scene:
a grey ground plane, a coloured sphere, and a free-fly camera you steer with the
mouse and `WASD`. You will write every line yourself, run it, and see the result.

This is a *lesson*, not a reference. Follow the steps in order. When you might
wonder *why*, there is a one-line answer with a link — keep moving.

## Prerequisites

- Rust 1.91+ (edition 2024) and a GPU with Vulkan, Metal, or DX12. Full setup is
  in [Contributing → Setup](../contributing/setup.md).
- A clone of the engine you can build:

```bash
git clone https://github.com/eraflo/KhoraEngine
cd KhoraEngine
cargo build
```

If `cargo build` finishes, you are ready.

## Step 1 — Create the game crate

Add a binary crate that depends on the SDK. The SDK is the **only** crate a game
needs — everything else is an implementation detail.

`Cargo.toml`:

```toml
[package]
name = "my-first-game"
version = "0.1.0"
edition = "2024"

[dependencies]
khora-sdk = { path = "crates/khora-sdk" }
anyhow = "1"
env_logger = "0.11"
```

> Why the SDK only? See [The SDK is the API](../concepts/saa.md).

## Step 2 — Write the skeleton

Open `src/main.rs`. Every Khora game is a struct that implements three traits:
**`EngineApp`** (lifecycle), **`AgentProvider`** (custom subsystems — none yet),
and **`PhaseProvider`** (custom frame phases — none yet). Start with the empty
shell:

```rust
use anyhow::Result;
use khora_sdk::prelude::math::{Quaternion, Vec3};
use khora_sdk::prelude::*;
use khora_sdk::run_winit;
use khora_sdk::winit_adapters::WinitWindowProvider;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, GameWorld, PhaseProvider, RenderSystem,
    Runtime, WgpuRenderSystem, WindowConfig,
};
use std::sync::{Arc, Mutex};

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

struct MyGame;

impl EngineApp for MyGame {
    fn window_config() -> WindowConfig {
        WindowConfig {
            title: "My First Khora Game".to_owned(),
            ..WindowConfig::default()
        }
    }

    fn new() -> Self {
        MyGame
    }

    fn setup(&mut self, world: &mut GameWorld, _runtime: &Runtime) {
        // Step 3 fills this in.
    }

    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {
        // The next tutorial fills this in.
    }
}

impl AgentProvider for MyGame {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}

impl PhaseProvider for MyGame {}
```

The `#[global_allocator]` line installs **`SaaTrackingAllocator`**, which feeds
the engine's memory heuristics. It is optional but recommended — see
[Telemetry](../concepts/saa.md).

> Why three traits? `setup`/`update` is for *game* logic; engine subsystems live
> in agents. See [Agents and Lanes](../concepts/agents-and-lanes.md).

## Step 3 — Spawn the scene

Fill in `setup`. It runs once, after engine initialisation, and gives you a
mutable [`GameWorld`]. You spawn entities through **`Vessel`**, a builder that
guarantees every entity has a `Transform` and `GlobalTransform`.

Replace the body of `setup` with:

```rust
fn setup(&mut self, world: &mut GameWorld, _runtime: &Runtime) {
    // A perspective camera, pulled back and turned to look into the scene.
    let camera = ecs::Camera::new_perspective(
        std::f32::consts::FRAC_PI_4, // 45° vertical FOV
        16.0 / 9.0,                  // aspect ratio
        0.1,                         // near plane
        1000.0,                      // far plane
    );
    khora_sdk::Vessel::at(world, Vec3::new(0.0, 2.0, 10.0))
        .with_component(camera)
        .with_rotation(Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::PI))
        .build();

    // A matte grey ground plane. A mesh needs a material to be drawn, so we
    // register one and attach its reference.
    let ground_mat = materials::StandardMaterial {
        base_color: math::LinearRgba::new(0.5, 0.5, 0.5, 1.0),
        roughness: 0.9,
        ..Default::default()
    };
    let ground_handle = world.add_material(ground_mat);
    khora_sdk::spawn_plane(world, 20.0, 0.0)
        .with_component(ground_handle)
        .build();

    // A directional "sun" light that casts shadows.
    let mut sun = ecs::Light::directional();
    if let ecs::LightType::Directional(ref mut d) = sun.light_type {
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

    // A glossy red sphere in front of the camera.
    let sphere_mat = materials::StandardMaterial {
        base_color: math::LinearRgba::RED,
        roughness: 0.2,
        ..Default::default()
    };
    let sphere_handle = world.add_material(sphere_mat);
    khora_sdk::spawn_sphere(world, 0.75, 32, 16)
        .at_position(Vec3::new(0.0, 0.5, -5.0))
        .with_component(sphere_handle)
        .build();
}
```

> Why an explicit material? The render projection has no default-material
> fallback, so a mesh with no material reference is skipped. See
> [Materials and lighting](../how-to/index.md).

`spawn_plane`, `spawn_sphere`, and `spawn_cube_at` return a `Vessel` you keep
building on; `add_material` returns a reference you attach with
`.with_component(...)`.

## Step 4 — Wire the backends and run

The engine is backend-agnostic: the renderer, physics, audio, and so on are
*your* choice, registered once in the **bootstrap closure** you pass to
**`run_winit`**. A first game needs only the renderer.

Add `main`:

```rust
fn main() -> Result<()> {
    env_logger::init();

    run_winit::<WinitWindowProvider, MyGame>(|window, runtime, _event_loop| {
        let mut rs = WgpuRenderSystem::new();
        rs.init(window).expect("renderer init failed");
        // The render agent reads the graphics device directly, so register it
        // before boxing the system.
        runtime.backends.insert(rs.graphics_device());
        let rs: Box<dyn RenderSystem> = Box::new(rs);
        runtime.backends.insert(Arc::new(Mutex::new(rs)));
    })?;
    Ok(())
}
```

`run_winit` is generic over a window provider (`WinitWindowProvider`, the default)
and your app type. The closure's `runtime` is where backends and resources are
registered before the frame loop starts.

> Why register the renderer yourself? Backends are swappable — lanes hold an
> abstract device and never know which one is underneath. See
> [Agents and Lanes](../concepts/agents-and-lanes.md).

Now run it:

```bash
cargo run --release -p my-first-game
```

**You should now see** a window titled *My First Khora Game* containing a grey
ground plane lit from above, with a glossy red sphere resting on it and a soft
shadow beneath. The terminal logs the engine starting up. The camera does not
move yet — that is the next lesson.

If the window is black, the most common cause is a missing material (the sphere
or plane will simply not draw); double-check Step 3. More fixes are in
[Troubleshooting](../how-to/troubleshoot.md).

## Next steps

You have a running, rendered scene. Now make it interactive:

- **[Adding behaviour](./adding-behaviour.md)** — move an entity with the keyboard,
  using the real per-frame delta time.
- [How-to recipes](../how-to/index.md) — load a mesh asset, tune materials and
  lighting, spawn primitives.
- Curious how the engine decides what to draw each frame? Read
  [The Symbiotic Adaptive Architecture](../concepts/saa.md).
