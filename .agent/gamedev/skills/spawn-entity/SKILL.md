---
name: spawn-entity
description: Spawns and configures entities in a Khora game — in the editor, from an Ergon script, or from Rust with the Vessel builder and spawn helpers. Use when adding objects, the player, props, lights, or cameras.
---

# Spawn an entity

Authored entities belong in a scene (the editor). Entities made while the game runs come from a script,
or from Rust.

## From a script (Ergon)
```text
component Health { int current = 100; int max = 100; }

behavior Spawner {
    Prefab? barrel;                      // set in the Inspector
    every 2s {
        Spawn(Position(), Health { current: 50 });
        Spawn(barrel, Health { max: 30 }, at: Position() + Vec3(0.0, 1.0, 0.0));
    }
}
```
`Spawn` returns the entity at once; it is alive from the frame's end. The prefab form places a linked
instance; a `null` prefab faults the turn, which then applies nothing.

## From Rust
```rust
use khora_sdk::prelude::ecs::{Camera, GlobalTransform, Light, LightType, Name, Transform};
use khora_sdk::prelude::materials::StandardMaterial;
use khora_sdk::prelude::math::{LinearRgba, Quaternion, Vec3};
use khora_sdk::{spawn_cube_at, spawn_plane, spawn_sphere, Vessel};

// Builder: every Vessel starts with a Transform and a GlobalTransform.
let camera = Vessel::at(world, Vec3::new(0.0, 2.0, 10.0))
    .with_component(Camera::new_perspective(std::f32::consts::FRAC_PI_4, 0.1, 1000.0))
    .with_rotation(Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::PI))
    .build();

// Primitives, each with a material (a mesh without one is not drawn).
let blue = world.add_material(StandardMaterial {
    base_color: LinearRgba::BLUE,
    roughness: 0.3,
    ..Default::default()
});
spawn_plane(world, 20.0, 0.0).with_component(blue.clone()).build();
spawn_cube_at(world, Vec3::new(2.0, 0.5, -3.0), 1.0).with_component(blue.clone()).build();
spawn_sphere(world, 0.75, 32, 16)
    .at_position(Vec3::new(0.0, 0.5, -5.0))
    .with_component(blue)
    .build();

// A light.
let mut sun = Light::directional();
if let LightType::Directional(ref mut d) = sun.light_type {
    d.intensity = 2.5;
    d.shadow_enabled = true;
}
Vessel::at(world, Vec3::new(0.0, 20.0, 5.0)).with_component(sun).build();

// A tuple, for full control: give a GlobalTransform beside the Transform.
world.spawn((Transform::from_translation(Vec3::Y), GlobalTransform::default(), Name::new("marker")));
```

Attach a script's behavior or component with `world.add_declared_component(e, "Name", &[…])`.

## Rules
- Move with `world.update_transform(e, |t| …)`, or `sync_global_transform(e)` after `get_transform_mut`.
- Reference assets by handle (`add_material`, `AssetHandle`); never inline raw asset data.
- Math via `prelude::math`.

Verify: Play (or `cargo run`) — the entity appears where expected.
