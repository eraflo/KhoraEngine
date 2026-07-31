# Add a flow

This guide shows you how to **implement a `Flow`** — a read-only projector that turns
the ECS World into a typed View for a domain's lanes — and register it with one line.

**Prerequisites:** you have read [Lanes](../concepts/agents-and-lanes.md) and understand that lanes
consume Views, not the World.

> A `Flow` is **read-only**. It never mutates the World. Representation changes (AGDF
> memory layout) are the data layer's own self-maintenance; semantic/gameplay changes
> are developer-authored systems — never a Flow. *Adapt the HOW, never the WHAT.*

## Step 1 — Define the View type

The View is the typed payload published into the `LaneBus`. It must be `Clone`,
`Send + Sync`, and `'static` — `Clone` lets the registration trampoline republish a
cached view cheaply. Hold `Arc`s and plain data, not borrows of the World.

```rust
#[derive(Clone)]
pub struct FoliageView {
    pub instances: Vec<FoliageInstance>,
}

#[derive(Clone)]
pub struct FoliageInstance {
    pub position: khora_core::math::Vec3,
    pub strength: f32,
}
```

## Step 2 — Implement the `Flow` trait

A Flow declares its `DOMAIN` and `NAME`, then runs in two read-only stages:
`select` narrows the relevant entities, `project` builds the View. Both receive the
engine `Runtime` so the Flow can read services its domain needs.

```rust
use khora_core::Runtime;
use khora_data::ecs::{SemanticDomain, World};
use khora_data::flow::{Flow, Selection};

#[derive(Default)]
pub struct FoliageFlow;

impl Flow for FoliageFlow {
    type View = FoliageView;

    const DOMAIN: SemanticDomain = SemanticDomain::Render;
    const NAME: &'static str = "foliage";

    fn select(&mut self, world: &World, _runtime: &Runtime) -> Selection {
        // Pick the entities this domain cares about (read-only).
        let mut sel = Selection::new();
        // … push relevant entities …
        let _ = world;
        sel
    }

    fn project(&self, world: &World, sel: &Selection, _runtime: &Runtime) -> FoliageView {
        // Build the typed View from the selected entities (read-only).
        let _ = (world, sel);
        FoliageView { instances: Vec::new() }
    }
}
```

## Step 3 — (Optional) add a `cache_key` for view reuse

If projection is expensive, override `cache_key`. When it returns `Some(k)` equal to
the previous tick's key, the trampoline republishes the cached View without re-running
`select`/`project`. The default (`None`) re-projects every tick — always correct, just
not cached.

A key **must fold in every input the projection reads**, or it serves stale views. Use
`combine_cache_key` over `World::instance_id`, the relevant `World::domain_epoch`s, and
a bit-level hash of any runtime state consulted. An over-broad key only re-projects
more often (safe); a key that misses an input is a bug.

```rust
fn cache_key(&self, world: &World, _runtime: &Runtime) -> Option<u64> {
    Some(khora_data::flow::combine_cache_key([
        world.instance_id(),
        world.domain_epoch(SemanticDomain::Render),
        world.domain_epoch(SemanticDomain::Spatial),
    ]))
}
```

## Step 4 — Register the Flow

One line wires the Flow into the Substrate Pass. `register_flow!` generates the
type-erased trampoline (single static instance, view cache included) and submits the
`FlowRegistration` via `inventory`:

```rust
use khora_data::register_flow;

register_flow!(FoliageFlow);
```

The lane reads the View off the bus (`ctx.get::<FoliageView>()` after the agent
inserts it) — see [Add a lane](./add-a-lane.md).

## Step 5 — Verify it works

```bash
cargo test --workspace
```

A unit test can drive `run_flow_cached` with a `World` and a `LaneBus`, then assert the
published View via `bus.get::<FoliageView>()` — and that mutating the relevant domain
invalidates the cache.

## Related

- [Lanes](../concepts/agents-and-lanes.md) — the bus / deck contract the Flow feeds.
- [AGDF](../concepts/agdf.md) — representation adaptation, the HOW a Flow must not change.
- [Add a lane](./add-a-lane.md) — consume the View this Flow produces.
