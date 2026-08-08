# Add a component

This guide shows you how to **define a new ECS component**, have it self-register
for serialization and the editor inspector, and query it from a system.

**Prerequisites:** you can build the workspace and have read
[ECS — CRPECS](../concepts/ecs.md).

## Step 1 — Define the struct with `#[derive(Component)]`

Components are plain data. Deriving `Component` generates the `Component` impl, a
`SerializableX` mirror with `From` conversions, and an `inventory` registration so
the type wires itself into the scene pipeline and the editor inspector — no manual
list to edit.

Declare the component's semantic domain with `#[component(domain = …)]`. The domain
drives change-epoch tracking (which `Flow`s must re-project when this component
changes). Valid domains: `Spatial`, `Render`, `Audio`, `Physics`, `Ui`, `Script`.

```rust
use khora_macros::Component;
use khora_core::math::Vec3; // engine math types only — never raw glam

/// Per-entity wind influence, sampled by the foliage system.
#[derive(Debug, Clone, Copy, PartialEq, Default, Component)]
#[component(domain = Spatial)]
pub struct WindAffected {
    pub direction: Vec3,
    pub strength: f32,
}
```

A `Default` impl is required: the editor's "add component" action and deserialization
of skipped fields both rely on it.

## Step 2 — Mark fields that must not be serialized

Use field attributes when a field cannot or should not round-trip through the
serializer:

- `#[component(skip)]` — exclude a single field (e.g. a GPU handle or runtime
  cache). It is omitted from the `Serializable` mirror and filled with
  `Default::default()` on load.
- `#[component(no_serializable)]` — applied to the **struct**, suppresses the generated
  mirror entirely. Use this only when you hand-write the serialization yourself.

```rust
#[derive(Debug, Clone, Default, Component)]
#[component(domain = Render)]
pub struct DecalProjector {
    pub size: f32,
    /// Runtime-only GPU handle — never serialized.
    #[component(skip)]
    pub texture: Option<u64>,
}
```

## Step 3 — Place the file and re-export it

Put the file next to the other components in
`crates/khora-data/src/ecs/components/` and add a `pub mod`/`pub use` line in that
module's `mod.rs`, matching the existing entries. The `inventory` registration fires
at startup automatically — there is nothing else to wire.

## Step 4 — Verify it works

Build, then confirm the component spawns and reads back:

```rust
let mut world = World::new();
let e = world.spawn(WindAffected { direction: Vec3::X, strength: 2.0 });
assert_eq!(world.get::<WindAffected>(e).unwrap().strength, 2.0);
```

```bash
cargo test --workspace
```

The component now appears in the editor inspector and survives scene save/load with
no further code.

## Related

- [ECS — CRPECS](../concepts/ecs.md) — storage model, queries, semantic domains.
- [Add a flow](./add-a-flow.md) — project this component into a View for lanes.
- [Conventions](../contributing/conventions.md) — one primary type per file, naming.
