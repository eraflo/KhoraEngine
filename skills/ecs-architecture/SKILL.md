---
name: ecs-architecture
description: "CRPECS ECS architecture — archetype-based Column-Row Partitioned Entity Component System with SoA storage, parallel queries, component registration, and entity lifecycle management. Use when working on ECS, World, components, queries, or archetypes."
license: Apache-2.0
metadata:
  author: eraflo
  version: "1.0.0"
  category: engine-development
---

# ECS Architecture (CRPECS)

## Instructions

When working on the ECS subsystem:

1. **Location**: `crates/khora-data/src/ecs/` contains the full ECS implementation.

2. **Core types**:
   - `World` — top-level container (entity storage + resource map)
   - `EntityId` — lightweight entity handle (generation + index)
   - `Archetype` — stores entities with identical component sets in SoA columns
   - `Component` trait — implemented via `#[derive(Component)]` from `khora-macros`
   - `ComponentBundle` — groups of components for batch spawning

3. **Key operations**:
   - `world.spawn(bundle)` → `EntityId`
   - `world.query::<(&T1, &T2)>()` → iterator over matching archetypes
   - `world.query_mut::<(&mut T1, &T2)>()` → mutable queries
   - `world.add_component(entity, component)` — triggers archetype migration
   - `world.remove_component::<T>(entity)` — triggers archetype migration

4. **Built-in components** (in `khora-data/src/ecs/components/`):
   - `Transform` — local position/rotation/scale
   - `GlobalTransform` — world-space computed transform
   - `Camera` — projection + view configuration
   - `Light` — light type (directional/point/spot), color, intensity, shadow config
   - `MaterialComponent` — material reference (handle)
   - `HandleComponent` — generic asset handle wrapper
   - `RigidBody` — physics body type, mass, velocity, CCD config
   - `Collider` — collision shape descriptor
   - `AudioSource` — audio clip, volume, spatial flags
   - `AudioListener` — listener position for 3D audio
   - `Parent` / `Children` — entity hierarchy relations

5. **UI components** (in `khora-data/src/ui/`):
   - `UiTransform` — position, size, anchoring
   - `UiColor` — background color (LinearRgba)
   - `UiText` — text content, font handle, color
   - `UiImage` — texture handle, scale mode
   - `UiBorder` — border width, color

6. **Semantic domains**: Logical groupings — `Render`, `Physics`, `UI` — for query optimization

7. **Architecture rules**:
   - Components must be `'static + Send + Sync`
   - Never store `EntityId` references that outlive the current frame
   - Use `CompactionLane` for archetype memory defragmentation
   - Queries are the primary data access pattern — avoid direct archetype manipulation

6. **Testing**: ~200+ ECS tests in `crates/khora-data/src/ecs/`

## Common Patterns

```rust
// Spawn an entity
let entity = world.spawn(ComponentBundle::new()
    .with(Transform::from_position(Vec3::new(0.0, 1.0, 0.0)))
    .with(MaterialComponent::new(material_handle)));

// Query entities
for (transform, material) in world.query::<(&Transform, &MaterialComponent)>() {
    // process...
}
```
