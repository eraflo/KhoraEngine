---
name: add-a-component
description: Defines and registers a new ECS component in the data layer. Use when adding gameplay or engine state that entities carry, with serialization handled by the derive macro.
---

# Add a component

Components live in `crates/khora-data/src/ecs/components/`. The `#[derive(Component)]` macro generates the
`SerializableX` mirror, `From` conversions, and `inventory` self-registration.

## Steps
1. Define the struct in the right `components/<domain>/` file:
   ```rust
   #[derive(Component)]
   #[component(domain = Render)]            // semantic domain
   pub struct MyThing {
       pub value: f32,
       #[component(skip)]                    // runtime-only, not serialized
       pub gpu: Option<TextureId>,
   }
   ```
2. Field attributes: `#[component(skip)]` for GPU handles / runtime caches;
   `#[component(no_serializable)]` on the type for unit structs / trait-object fields needing a manual mirror.
3. The derive self-registers via `inventory`. For batch registration use `register_components!` in
   `crates/khora-data/src/ecs/components/registrations.rs`. Only generics (`HandleComponent<T>`) and
   hand-written impls stay explicit in `World::new`.
4. If the component should appear in the editor inspector, confirm its `ComponentRegistration` is picked up
   (the macro emits add/remove function pointers).
5. Add `#[cfg(test)]` round-trip serialization tests.

## Rules
- Components are `'static + Send + Sync`. Don't store raw asset data inline — use `AssetHandle<T>` /
  `HandleComponent<T>`.

## Verify
`cargo test --workspace`. Delegate storage/layout questions to the `ecs-data-expert` agent.
