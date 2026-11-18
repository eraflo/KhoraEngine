# Add a component

This guide shows you how to **define a new engine component**: data entities
carry, registered for saving, the inspector and scripts by one derive.

**Prerequisites:** you can build the workspace and have read
[Data and the ECS](../concepts/ecs.md).

> This is how the **engine** gains a component, in `khora-data`: the derive writes
> paths into that crate, so it works there only. A game declares its own
> components in Ergon (`component Health { … }`, see
> [Scripting](../concepts/scripting/index.md)); they are stored, saved and
> inspected the same way.

## Step 1 — Define the struct with `#[derive(Component)]`

A component is plain data. Deriving `Component` writes the `Component` impl, a
serialisable mirror (`SerializableX`) with `From` conversions both ways, and an
`inventory` registration, so the type reaches the world, the scene files and the
inspector with no list to edit.

Give it a **domain** with `#[component(domain = …)]`: `Spatial`, `Render`,
`Audio`, `Physics`, `Ui` or `Script`. The domain decides which pages store it and
whose cached Views a change to it invalidates. It is not optional in practice: a
component with no domain is not registered, and `World::spawn` refuses it.

```rust
use khora_core::math::Vec3; // engine math types only — never raw glam
use khora_macros::Component;

/// Per-entity wind influence, sampled by the foliage flow.
#[derive(Debug, Clone, Copy, PartialEq, Default, Component)]
#[component(domain = Spatial)]
pub struct WindAffected {
    pub direction: Vec3,
    pub strength: f32,
}
```

The struct needs `Default`: the registration creates a default value for the
inspector's *Add Component* and for a load.

> The derive checks every `#[component(…)]` key and fails the build on the key
> when it is wrong. The type takes `domain`, `provenance`, `formerly`,
> `resumable`, `no_serializable`, `layout`, `check`; a field takes `skip` and
> `formerly`. A key it does not know, a key at the wrong level, a key given
> twice (a `formerly` naming the same old name twice), a flag given a value, or a
> value of the wrong kind is a compile error that names the keys it takes. So is
> a key that `no_serializable` leaves with nothing to act on: `check`,
> `provenance`, `resumable` and `formerly` (on the type or a field) all work
> through the registration it removes.
>
> ```text
> error: unknown `#[component]` key `domian` on the type: its keys are `domain`, …
>   |
> 4 | #[component(domian = Physics)]
>   |             ^^^^^^
> ```

## Step 2 — Mark fields that must not be saved

`#[component(skip)]` on a field — a GPU handle, a cache — leaves it out of the
mirror: it is never saved, and a load fills it with its type's `Default`.

```rust
#[derive(Debug, Clone, Default, Component)]
#[component(domain = Render)]
pub struct DecalProjector {
    pub size: f32,
    /// Filled by the engine once the texture is resident; never saved.
    #[component(skip)]
    pub texture: Option<u64>,
}
```

`#[component(no_serializable)]` on the struct removes the registration
altogether: the component is never saved, never shown in the inspector and never
reachable by name. That is not the way to say "runtime state" — a provenance is
(Step 3). No shipped component uses it.

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
takes `resumable` as well — no Rust component does; a behavior's state
companion, declared while the engine runs, is resumable. Marking another one is
a decision about how its domain reads it back on load; see
[Scenes and game saves](../concepts/saves.md).

```rust
#[derive(Debug, Clone, Default, Component)]
#[component(domain = Spatial, provenance = Derived)]
pub struct WindSample {
    pub gust: f32,
}
```

A value the engine recomputes every frame is written with `set_if_neq`, so a
frame that changes nothing writes nothing (see
[Knowing what changed](../concepts/change-detection.md)).

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

## Step 5 — Name the values the engine cannot use

When a field can hold a value the engine cannot work with — a strength below
zero, a direction of length zero — give the type a check:
`#[component(check = WindAffected::check)]`, a `fn(&Self) -> Vec<FieldProblem>`
naming each bad field by its dotted path in the component's JSON form.

```rust
use khora_core::problems::ProblemSeverity;
use crate::ecs::FieldProblem;

#[derive(Debug, Clone, Copy, PartialEq, Default, Component)]
#[component(domain = Spatial, check = WindAffected::check)]
pub struct WindAffected {
    pub direction: Vec3,
    pub strength: f32,
}

impl WindAffected {
    /// Returns the fields the wind cannot use.
    pub fn check(&self) -> Vec<FieldProblem> {
        let mut problems = Vec::new();
        if !(self.strength >= 0.0) {
            problems.push(FieldProblem {
                field: "strength",
                severity: ProblemSeverity::Error,
                message: format!("strength must be 0 or more (it is {})", self.strength),
            });
        }
        problems
    }
}
```

A check **never rewrites nor refuses** a value. A load keeps it and reports it as
`Unusable`; while the engine runs, the `component_checks` system reports it to
`Problems`; the inspector shows the message under the field. Whatever is built from
the component must then do nothing with a bad value, never panic on it.
`Camera::check` and `Light::check` are examples.

## Step 6 — Place the file and re-export it

Put the file next to the other components in
`crates/khora-data/src/ecs/components/`, and add `mod wind_affected;` and
`pub use wind_affected::*;` to that module's `mod.rs`, beside the existing
entries. The registration is collected at startup; there is nothing else to wire.

A compute-heavy component whose named fields are all `f32` can opt into a
field-split column with `#[component(layout = "soa")]` (`"aos"`, the default, is
the only other value); it is then read by value,
through the `Soa<T>` query term. See [AGDF](../concepts/agdf.md#layer-2--the-field-soa-substrate-and-why-it-is-the-lever)
before choosing it.

## Step 7 — Verify it works

Confirm the component spawns and reads back:

```rust
let mut world = World::new();
let e = world.spawn(WindAffected { direction: Vec3::X, strength: 2.0 });
assert_eq!(world.get::<WindAffected>(e).map(|w| w.strength), Some(2.0));
```

```bash
cargo t -p khora-data
```

The component now appears in the inspector's *Add Component*, survives a scene
save and load, and is readable by name from a script.

## Related

- [Data and the ECS](../concepts/ecs.md) — storage, queries, semantic domains.
- [Add a flow](./add-a-flow.md) — project this component into a View for lanes.
- [Conventions](../contributing/conventions.md) — one primary type per file, naming.
