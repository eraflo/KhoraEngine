# 13. The Khora SDK Engine API

As the Khora Engine grew in complexity with its intricate **SAA (Symbiotic Adaptive Architecture)** and **CLAD (Control, Lanes, Agents, Data)** pattern, it became clear that exposing the internal systems directly to game developers would be overwhelming and architecturally impure.

To solve this, we introduced the **`khora-sdk`** crate.

## The SDK Principle

The `khora-sdk` crate is designed to be the **absolute single point of entry** for any external project or game using the Khora Engine.

> [!WARNING]
> External projects (like the `sandbox` example) should **never** declare direct dependencies on internal crates like `khora-core`, `khora-data`, or `khora-lanes`. Doing so breaks the engine's architectural encapsulation.

If a developer needs access to a core type (like a `Vec3` from `khora_core`, or a `Transform` from `khora_data`), the `khora-sdk` crate is responsible for **re-exporting** those types.

### The `prelude` Module

The SDK provides a highly convenient `prelude` module that re-exports the most commonly used types across the entire engine architecture.

```rust
use khora_sdk::prelude::*;
use khora_sdk::prelude::math::{Vec3, Quaternion, LinearRgba};
use khora_sdk::prelude::ecs::{EntityId, Transform, GlobalTransform, Camera, Light};
use khora_sdk::prelude::materials::StandardMaterial;
```

This ensures that the underlying implementation crates (`khora-core`, `khora-data`) can be completely refactored or replaced without ever breaking the user's game code, as long as the SDK continues to export the expected interface.

## The `Vessel` Abstraction

To interact with the ECS (Entity Component System) without needing to understand the underlying archetype storage or direct component manipulation, the SDK provides the `Vessel` abstraction.

A `Vessel` is a high-level, builder-pattern wrapper around an ECS entity. It guarantees that every object in your game world has a basic physical presence (a `Transform` for local coordinates and a `GlobalTransform` for rendering).

### Example Usage

```rust
// Spawning a 3D Sphere in the world with a PBR material
let gold_material = StandardMaterial {
    base_color: LinearRgba::new(1.0, 0.8, 0.4, 1.0),
    metallic: 1.0,
    roughness: 0.2,
    ..Default::default()
};

let material_handle = world.add_material(Box::new(gold_material));

khora_sdk::spawn_sphere(world, 1.0, 32, 16)
    .at_position(Vec3::new(0.0, 5.0, -10.0))
    .with_component(material_handle)
    .build();
```

## The GameWorld and Application Traits

The SDK provides a standard `Application` trait that developers must implement. This trait isolates game logic from the low-level render loop and OS windowing events. The `GameWorld` is provided as a mutable parameter to `setup` and `update` loops, serving as the interface to spawn entities, define lighting, and react to inputs.
