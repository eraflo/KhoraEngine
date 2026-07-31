---
name: spawn-entity
description: Spawns and configures entities in a Khora game using the Vessel builder and spawn helpers. Use when adding objects, the player, props, lights, or cameras to a scene.
---

# Spawn an entity

Use the `Vessel` builder or the primitive helpers — never poke ECS storage directly.

```rust
// Builder:
let e = Vessel::at(world, Vec3::new(x, y, z))
    .with_component(component)                 // Camera, Light, MaterialComponent, RigidBody, …
    .with_rotation(Quaternion::from_axis_angle(Vec3::Y, angle))
    .build();

// Primitives:
spawn_plane(world, size, height).build();
spawn_cube_at(world, Vec3::new(x, y, z)).build();
spawn_sphere(world, radius, sectors, stacks).at_position(pos).with_component(mat_handle).build();

// Materials:
let mat = world.add_material(StandardMaterial { base_color: LinearRgba::BLUE, roughness: 0.3, ..Default::default() });

// Raw tuple spawn when you need full control:
world.spawn((Transform::from_translation(pos), GlobalTransform::default(), my_component));
```

## Rules
- Add a `GlobalTransform` alongside `Transform` when spawning by tuple. Call `world.sync_global_transform(e)`
  after moving an entity whose world transform matters this frame.
- Reference assets via handles (`add_material`) — never store raw asset data inline.
- Math via `prelude::math`.

Verify with `cargo run` — the entity appears where expected.
