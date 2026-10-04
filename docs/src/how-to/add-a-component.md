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
- `#[component(no_serializable)]` — applied to the **struct**, drops the generated
  registration entirely: the component is never saved and never shown in the
  inspector. Use it for state that has no business in either, or when you submit
  a hand-written `ComponentRegistration` yourself.

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

## Step 3 — Say who writes it

By default a component is `Authored`: written by a person or by game code, saved in
the scene, copied when an entity is duplicated, and offered in the inspector's
"+ Add Component". When that is not true, say so on the struct with
`#[component(provenance = …)]`:

| Provenance | Written by | In a scene | Copied on duplicate | Offered in "+ Add" |
|---|---|---|---|---|
| `Authored` (default) | an author or game code | yes | yes | yes |
| `ToolAuthored` | a tool action (`Parent`, set by dragging) | yes | yes | no |
| `Derived` | the engine, from authored state (`GlobalTransform`) | no | no | no |
| `Runtime` | the engine, while running (`PhysicsDebugData`) | no | no | no |

Runtime state that a **game save** must keep for play to resume where it stopped
takes `resumable` as well — `ScriptState` is the one such component today. Marking
another one is a decision about how its domain reads it back on load; see
[Scenes and game saves](../concepts/saves.md).

```rust
#[derive(Debug, Clone, Default, Component)]
#[component(domain = Spatial, provenance = Derived)]
pub struct WindSample {
    pub gust: f32,
}
```

## Step 4 — Rename and remove by declaration

Scenes and saves hold components and fields **by name**, so a rename is not free:
a save written under the old name must still load. Declare it, and the load reads
the old name as today's and reports the rename in its `LoadReport`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Default, Component)]
#[component(domain = Spatial, formerly = "WindInfluence")] // the type's old name
pub struct WindAffected {
    pub direction: Vec3,
    #[component(formerly = "power")] // the field's old name
    pub strength: f32,
}
```

A field added since a save takes its default; a field removed is dropped; both are
reported, neither is an error. Removing a whole component type is different — a
save that names a type the build no longer has fails to load, on purpose, unless
the removal was declared:

```rust
inventory::submit! {
    khora_data::scene::RetiredComponent { name: "WindGust" }
}
```

A retired component is skipped on load and reported. Never write a payload
migration; the names are the migration. See
[Serialization](../concepts/serialization.md).

## Step 5 — Place the file and re-export it

Put the file next to the other components in
`crates/khora-data/src/ecs/components/` and add a `pub mod`/`pub use` line in that
module's `mod.rs`, matching the existing entries. The `inventory` registration fires
at startup automatically — there is nothing else to wire.

## Step 6 — Verify it works

Build, then confirm the component spawns and reads back:

```rust
let mut world = World::new();
let e = world.spawn(WindAffected { direction: Vec3::X, strength: 2.0 });
assert_eq!(world.get::<WindAffected>(e).unwrap().strength, 2.0);
```

```bash
cargo gate
```

The component now appears in the editor inspector and survives scene save/load with
no further code.

## Related

- [ECS — CRPECS](../concepts/ecs.md) — storage model, queries, semantic domains.
- [Add a flow](./add-a-flow.md) — project this component into a View for lanes.
- [Conventions](../contributing/conventions.md) — one primary type per file, naming.
