# Spawn entities and move them

This guide shows you how to spawn entities, read and update their `Transform`, and
build a parent–child hierarchy — the everyday building blocks of a Khora scene.

> **Prerequisites.** You have completed [Your first game](../tutorials/your-first-game.md)
> and have an `EngineApp` with a `setup` / `update` loop and a `GameWorld`.

## Spawn an entity with the `Vessel` builder

`Vessel` spawns an entity already carrying a `Transform` and a `GlobalTransform`, then
lets you configure it with chained calls and finalize with `build()`.

```rust
use khora_sdk::Vessel;
use khora_sdk::prelude::ecs::Name;
use khora_sdk::prelude::math::{Quaternion, Vec3};

let crate_entity = Vessel::at(world, Vec3::new(1.0, 0.0, -3.0))
    .with_scale(Vec3::ONE * 2.0)
    .with_rotation(Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2))
    .with_component(Name::new("crate"))
    .build();
```

The available builder steps are `at(world, pos)` / `new(world)` to create it,
`at_position`, `with_rotation`, `with_scale`, `with_transform`, and `with_component`
for any extra component. `build()` returns the `EntityId`.

For built-in shapes use the procedural helpers — they attach the mesh for you and
return a `Vessel` you can keep configuring:

```rust
use khora_sdk::{spawn_cube_at, spawn_plane, spawn_sphere};

let ground = spawn_plane(world, 20.0, 0.0).build();          // size, y
let ball   = spawn_sphere(world, 0.75, 32, 16)               // radius, segments, rings
    .at_position(Vec3::new(0.0, 0.5, -5.0))
    .build();
let block  = spawn_cube_at(world, Vec3::new(2.0, 0.5, -4.0), 1.0).build();
```

## Spawn from a component bundle

When you want full control over which components an entity starts with, spawn a tuple
directly.

```rust
use khora_sdk::prelude::ecs::{GlobalTransform, Transform};

let entity = world.spawn((
    Transform::from_translation(Vec3::new(0.0, 1.0, -2.0)),
    GlobalTransform::default(),
));
```

`GlobalTransform` is the world pose the renderer, audio and physics read. An entity
spawned with a `Transform` alone gets one from the engine after your `update`
(`ensure_global_transform`, first in `PostSimulation`), seeded from its local pose, so it
is drawn from the same frame — an entity loaded from a scene too, since a scene never
holds one. Giving it yourself, as above, lets code that runs earlier in the frame read
it.

## Read and update a Transform

`Transform` exposes `translation`, `rotation`, and `scale` directly. Read with
`get_transform`, mutate with `update_transform` (which re-syncs `GlobalTransform`
for you):

```rust
// Read
if let Some(t) = world.get_transform(entity) {
    let _pos = t.translation;
}

// Update + auto-sync the GlobalTransform the renderer reads
world.update_transform(entity, |t| {
    t.translation = t.translation + Vec3::Y;
});
```

If you mutate a transform through `get_transform_mut` instead, call
`world.sync_global_transform(entity)` afterwards so the change reaches the renderer —
this is what the sandbox's player controller does each frame.

To touch many entities at once, query mutably:

```rust
use khora_sdk::prelude::ecs::Transform;

for (mut t,) in world.query_mut::<(&mut Transform,)>() {
    t.translation = t.translation + Vec3::Y * 0.01;
}
```

> **Why two transforms?** `Transform` is the local pose you author; `GlobalTransform`
> is the world-space matrix the render and audio paths consume. See
> [Data and the ECS](../concepts/ecs.md) for the full story.

## Build a parent–child hierarchy

Reparent with `set_parent`. It maintains both the `Parent` component on the child and
the `Children` list on the parent, and refuses cycles silently.

```rust
let turret = spawn_cube_at(world, Vec3::new(0.0, 1.0, 0.0), 0.5).build();
let barrel = spawn_cube_at(world, Vec3::new(0.0, 1.2, 0.5), 0.2).build();

world.set_parent(barrel, Some(turret)); // attach
world.set_parent(barrel, None);         // detach back to a root
```

## Read a direction or a rotation from a scaled pose

A world pose can be scaled, even unevenly or mirrored, so its matrix is not a rotation.
`GlobalTransform` (an `AffineTransform`) reads both apart:

- `rotation()` is the turn alone, whatever the scale. A mirror is put on the first axis,
  and an axis scaled to zero is rebuilt from the two others.
- `transform_direction(local)` is the unit world direction `local` points along. Under an
  uneven scale it leans with the scaled axes, as the drawn geometry does: a diagonal arrow
  on a body squashed along X points where the drawn arrow points. (A surface normal is
  not such a direction: it needs the inverse transpose.)

```rust
if let Some(global) = world.get_component::<GlobalTransform>(entity) {
    let facing = global.0.transform_direction(Vec3::new(0.0, 0.0, -1.0));
    let turn = global.0.rotation();
}
```

The cases they handle, and why, are in [Transform maths](../reference/math.md).

## Expected result

`build()` / `spawn` return a live `EntityId`; `get_transform(entity)` returns the pose
you set, and updated transforms become visible to the renderer after the sync. Parented
entities appear in their parent's `Children` list.

## Related

- [Materials and lighting](./materials-and-lighting.md) — make spawned meshes visible.
- [Map input to actions](./map-input.md) — drive a transform from the keyboard.
- [`GameWorld` and `Vessel` reference](../reference/sdk.md) — the full method list.
