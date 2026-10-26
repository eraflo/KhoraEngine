---
name: add-a-lane
description: Adds a new hot-path Lane to the engine (render, physics, audio, or UI). Use when implementing a new rendering technique, physics strategy, or any per-frame pipeline step that an Agent should be able to select.
---

# Add a Lane

A `Lane` is a unit of hot-path work selected by an `Agent` via GORNA. Trait in
`crates/khora-core/src/lane/mod.rs`.

## Steps
1. Create `crates/khora-lanes/src/<domain>_lane/<name>_lane.rs`. Implement `Lane`:
   lifecycle (`on_initialize`, `execute`, `on_shutdown` — release every GPU resource `on_initialize` created) + strategy metadata
   (`strategy_name`, `lane_kind`, `estimate_cost`).
2. **Hold only persistent state.** Put init/render bodies in private free functions in the module;
   the trait impl dispatches to them. Construct via `Default`.
3. **Read inputs from the `LaneContext`** the agent filled (the bus `View` it inserted, via `get_ref`),
   **write outputs into the deck it lent** (`ctx.slot::<OutputDeck>()`).
   Never call `world.query*` from a lane.
4. If render: name the pipeline (`khora::pipelines::<name>`) in the lane's pipeline spec; respect
   the **4-bind-group** budget (see [`add-a-shader`](../add-a-shader/SKILL.md)).
5. Register the lane with its agent (one agent per `LaneKind`) and add it to the agent's strategy set if it
   is a selectable strategy.
6. Add `#[cfg(test)]` unit tests.

## Rules
- Never bypass the `Lane` abstraction for hot-path work. Never expose raw GPU handles. No `unwrap()` on GPU/IO.

## Verify
`cargo gate`; for render lanes, `cargo run -p sandbox` (clean frame). Delegate domain detail to
the matching specialist agent (graphics / physics / audio).
