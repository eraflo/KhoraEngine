# Add a flow

This guide shows you how to **write a `Flow`** — a read-only projector that turns
the `World` into a typed View for a family's lanes — and register it with one
line.

**Prerequisites:** you have read [Agents and Lanes](../concepts/agents-and-lanes.md):
a lane never reads the `World`, it reads Views from the `LaneBus`. A Flow is what
publishes them.

> A `Flow` is **read-only**. It never changes the `World`. A change of layout is
> the data layer's own business, and a change of what entities hold is the game's —
> never a Flow's. *Adapt the HOW, never the WHAT.*

## Step 1 — Define the View

The View is what the Flow publishes on the bus each frame. It must be
`Clone + Send + Sync + 'static`: `Clone` so a cached View is published again
without projecting. Hold plain data and `Arc`s, never a borrow of the `World`.

```rust
use khora_core::math::Vec3;

/// The foliage instances the wind lanes sway.
#[derive(Clone, Default)]
pub struct FoliageView {
    pub instances: Vec<FoliageInstance>,
}

#[derive(Clone)]
pub struct FoliageInstance {
    pub position: Vec3,
    pub strength: f32,
}
```

## Step 2 — Implement the `Flow` trait

The trait (`khora-data/src/flow/mod.rs`):

```rust
pub trait Flow: Send + Sync {
    type View: Any + Send + Sync + Clone + 'static;
    const DOMAIN: SemanticDomain;
    const NAME: &'static str;
    const ROLE: Role;

    fn select(&mut self, world: &World, runtime: &Runtime, ctx: FlowContext) -> Selection { … }
    fn project(&self, world: &World, sel: &Selection, runtime: &Runtime) -> Self::View;
    fn cache_key(&self, world: &World, runtime: &Runtime) -> Option<u64> { None }
}
```

- `DOMAIN` is the domain the Flow projects for; `NAME` names it in telemetry.
- `ROLE` has no default. `Role::Upkeep` runs every frame; `Role::Serves(FAMILY)`
  runs only while that family is on, and the Flow is skipped (publishing nothing)
  while it is off.
- `select` narrows the entities to project. Its default selects within the
  family's **scope**: when a family is limited to the entities holding a marker
  component (`FlowContext::scope`), the selection `admits` only those. Override it
  only to narrow further, starting from `Selection::new().within(world, ctx.scope)`
  so the scope is kept.
- `project` builds the View, reading only. A scoped Flow keeps to
  `sel.admits(entity)`.

```rust
use khora_core::ecs::entity::EntityId;
use khora_core::Runtime;
use khora_data::ecs::{GlobalTransform, Role, SemanticDomain, World};
use khora_data::flow::{Flow, Selection};

#[derive(Default)]
pub struct FoliageFlow;

impl Flow for FoliageFlow {
    type View = FoliageView;

    const DOMAIN: SemanticDomain = SemanticDomain::Render;
    const NAME: &'static str = "foliage";
    const ROLE: Role = Role::Upkeep;

    fn project(&self, world: &World, sel: &Selection, _runtime: &Runtime) -> FoliageView {
        let instances = world
            .query::<(EntityId, &GlobalTransform, &WindAffected)>()
            .filter(|(entity, _, _)| sel.admits(*entity))
            .map(|(_, global, wind)| FoliageInstance {
                position: global.0.translation(),
                strength: wind.strength,
            })
            .collect();
        FoliageView { instances }
    }
}
```

`WindAffected` is the component of [Add a component](./add-a-component.md). The
Flow type needs `Default`: the registration builds its one instance with it.

## Step 3 — (Optional) add a `cache_key`

If projecting is costly, override `cache_key`. When it returns `Some(k)` equal to
the previous run's key, the runner publishes the kept View again and runs neither
`select` nor `project`. The default, `None`, projects every frame: always correct,
never cached.

The key **must fold in every input the projection reads**, or it serves stale
Views. Use `combine_cache_key` over `World::instance_id`, the
`World::domain_epoch` of every domain the projection reads, and a bit-level hash
of any runtime value it reads. A domain's epoch moves when one of its values is
written or one of its rows moves — not when writable access is merely handed out
(see [Knowing what changed](../concepts/change-detection.md#a-whole-domain-at-once)).
The runner folds the family's scope into the key itself.

```rust
fn cache_key(&self, world: &World, _runtime: &Runtime) -> Option<u64> {
    Some(khora_data::flow::combine_cache_key([
        world.instance_id(),
        world.domain_epoch(SemanticDomain::Spatial), // GlobalTransform, WindAffected
    ]))
}
```

Here both components the projection reads are spatial, so one epoch covers them.
A key with an input too many only projects more often; a key that misses an input
is a bug. See [AGDF — Flow view caching](../concepts/agdf.md#flow-view-caching--per-domain-change-epochs)
for why a hit is safe.

## Step 4 — Register the Flow

One line wires it into the frame. `register_flow!` writes the runner — one
instance and its cache, behind one lock — and submits its `FlowRegistration`
through `inventory`:

```rust
khora_data::register_flow!(FoliageFlow);
```

A Flow that reads `Changed<T>` names the components it observes, so their row
ticks are kept for it:

```rust
khora_data::register_flow!(FoliageFlow, observes = [WindAffected]);
```

Put the file in `crates/khora-data/src/flow/` and re-export the Flow and its View
from that module's `mod.rs`, beside the others. Every registered Flow runs in the
**Substrate Pass**: after the frame's `PreExtract` systems, before any agent. Flows
are not budgeted; only families compete for the frame.

## Step 5 — Read the View from a lane

A lane names what it reads in its inputs struct. A View is a `#[bus]` field:

```rust
use khora_macros::LaneInputs;

#[derive(LaneInputs)]
pub struct WindInputs<'f> {
    /// This frame's foliage, from `FoliageFlow`.
    #[bus]
    pub foliage: &'f FoliageView,
}
```

The lane receives the struct in `record`. A bus read is refused, and the lane
does not run, when the bus holds no such View (`Missing`) or holds one published
for another frame (`Stale`). See [Add a lane](./add-a-lane.md).

## Step 6 — Verify it works

Drive the runner directly, as the engine's own tests do
(`khora-data/src/flow/registration.rs`):

```rust
let mut world = World::new();
let runtime = Runtime::new();
let (mut flow, mut cache) = (FoliageFlow, None);

let mut bus = LaneBus::new();
khora_data::flow::run_flow_cached(
    &mut flow, &mut cache, &mut world, &mut bus, &runtime, FlowContext::default(),
);
assert!(bus.get::<FoliageView>().is_some());
```

Then write to a component the Flow reads, run it again, and check that the View
changed — and that, with nothing written, it was published from the cache.

```bash
cargo t -p khora-data flow
```

## Related

- [Agents and Lanes](../concepts/agents-and-lanes.md) — the bus and the lanes that
  read it.
- [AGDF](../concepts/agdf.md) — view caching, and the HOW a Flow must not change.
- [Add a lane](./add-a-lane.md) — consume the View this Flow produces.
