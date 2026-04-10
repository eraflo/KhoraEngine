# SDK Developer Guide

The Khora SDK provides a clean, minimal API for game developers. The engine manages complexity internally — you focus on your game.

## Quick Start

```rust
use khora_sdk::prelude::*;
use khora_sdk::{Application, Engine, AppContext, GameWorld, InputEvent};

struct MyGame {
    player: Option<EntityId>,
}

impl Application for MyGame {
    fn new() -> Self {
        Self { player: None }
    }

    fn setup(&mut self, world: &mut GameWorld, ctx: &mut AppContext) {
        // Spawn initial entities
        let cam = Camera::new_perspective(std::f32::consts::FRAC_PI_4, 16.0/9.0, 0.1, 1000.0);
        world.spawn((
            Transform::default(),
            GlobalTransform::identity(),
            cam,
            Name::new("Main Camera"),
        ));
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        // Game logic here
    }
}

fn main() -> anyhow::Result<()> {
    Engine::run::<MyGame>()
}
```

## Application Trait

| Method | When | Purpose |
|--------|------|---------|
| `new()` | Once, at boot | Construct your game struct |
| `setup(world, ctx)` | Once, after init | Spawn initial entities, cache services |
| `update(world, inputs)` | Every frame | Game logic |
| `render()` | Every frame | Produce render objects (optional) |

## AppContext

```rust
pub struct AppContext {
    pub services: Arc<ServiceRegistry>,
}
```

Access engine services during `setup()`:

```rust
fn setup(&mut self, world: &mut GameWorld, ctx: &mut AppContext) {
    if let Some(device) = ctx.services.get::<Arc<dyn GraphicsDevice>>() {
        // Use the graphics device
    }
}
```

## GameWorld

The safe interface to the ECS:

```rust
// Spawn entities
let entity = world.spawn((Transform::default(), Name::new("Player")));

// Query components
for (transform, mut global) in world.query::<(&Transform, &mut GlobalTransform)>() {
    // ...
}

// Get single component
if let Some(name) = world.get_component::<Name>(entity) {
    println!("Entity name: {}", name.0);
}

// Despawn
world.despawn(entity);
```

## Prelude

The SDK prelude provides common types grouped by category:

```rust
use khora_sdk::prelude::*;          // Everything
use khora_sdk::prelude::ecs::*;     // ECS types only
use khora_sdk::prelude::math::*;    // Math types only
use khora_sdk::prelude::materials::*; // Material types only
```

> [!TIP]
> **Keep it simple.** The SDK is intentionally minimal. Complex engine internals (agents, lanes, scheduler) are hidden. If you need low-level access, use the internal crates directly.
