# Add a lane

This guide shows you how to **implement a `Lane`** — a swappable hot-path strategy —
have an agent select it, feed it a View from the `LaneBus`, and collect its output
through the `OutputDeck`.

**Prerequisites:** you have completed
[Extending the engine](../tutorials/extending-the-engine.md) and read
[Lanes](../concepts/agents-and-lanes.md).

## Step 1 — Implement the `Lane` trait

A lane provides identity (`strategy_name`, `lane_kind`), an optional cost estimate,
and the work in `execute`. Note `execute` takes `&self` (not `&mut self`) and returns
`Result<(), LaneError>` — lanes hold no per-frame mutable state of their own; their
inputs and outputs flow through the `LaneContext`.

```rust
use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind};

#[derive(Default)]
pub struct LowDetailFoliageLane;

impl Lane for LowDetailFoliageLane {
    fn strategy_name(&self) -> &'static str {
        "LowDetailFoliage"
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Render
    }

    fn estimate_cost(&self, _ctx: &LaneContext) -> f32 {
        0.4 // cheaper than the full-detail strategy — GORNA can prefer it under load
    }

    fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        // Read the typed inputs the agent inserted; fail cleanly if absent.
        let view = ctx
            .get::<FoliageView>()
            .ok_or_else(|| LaneError::missing("FoliageView"))?;
        // … record draw commands from `view` …
        let _ = view;
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
```

> `FoliageView` here is the typed projection a `Flow` publishes — see
> [Add a flow](./add-a-flow.md). The agent reads it off the bus and threads it into
> the `LaneContext`; the lane never queries the World.

## Step 2 — Feed the lane a View from the `LaneBus`

Lanes do **not** read the `LaneBus` directly. The owning agent reads the View off
`EngineContext::bus`, then inserts it (by value, or borrowed via `Ref` for the frame)
into a fresh `LaneContext`, and dispatches `Lane::execute`:

```rust
// inside the agent's `execute(&mut self, context: &mut EngineContext<'_>)`
let Some(view) = context.bus.get::<FoliageView>() else { return };

let mut ctx = LaneContext::new();
ctx.insert(view.clone()); // FoliageView: Clone, so insert by value
if let Some(lane) = self.lane.as_ref() {
    if let Err(e) = lane.execute(&mut ctx) {
        log::error!("foliage lane failed: {e}");
    }
}
```

This is the boundary the architecture enforces: **lanes consume Views, the `Flow`
produces them.** Querying the World from a lane is a rule violation.

## Step 3 — Write outputs to the `OutputDeck`

For outputs the engine drains at the I/O boundary (recorded command buffers, draw
lists), write into a typed slot on the `OutputDeck`. The agent owns the deck via
`EngineContext::deck`; thread a `Slot` into the `LaneContext` if the lane itself
accumulates output. `OutputDeck::slot::<T>()` lazily creates a `T::default()` on
first access:

```rust
// agent side
let draws: &mut Vec<DrawCommand> = context.deck.slot::<Vec<DrawCommand>>();
// … or hand the lane a Slot into it and let the lane push …
```

## Step 4 — Register the lane on an agent

Lanes are held in the agent's `LaneRegistry` (or a small set of `Option<Box<dyn
Lane>>` fields, as the built-in agents do). The agent constructs its lanes in
`on_initialize`, then selects one per frame in `apply_budget` based on the GORNA
strategy. A lane runs its one-shot setup through `Lane::on_initialize` if it needs a
device:

```rust
// inside the agent's on_initialize
self.lane = Some(Box::new(LowDetailFoliageLane::default()));
```

To add a whole new subsystem rather than a strategy for an existing one, see
[Add an agent](./add-an-agent.md).

## Step 5 — Verify it works

```bash
cargo test --workspace
cargo run -p sandbox   # confirm no wgpu/Vulkan validation errors for a render lane
```

A unit test can exercise the lane directly by building a `LaneContext`, inserting a
stub view, and asserting `execute` returns `Ok(())`.

## Related

- [Lanes](../concepts/agents-and-lanes.md) — the lane lifecycle and the two-level trait hierarchy.
- [Add a flow](./add-a-flow.md) — produce the View this lane consumes.
- [Add an agent](./add-an-agent.md) — the strategist that selects this lane.
