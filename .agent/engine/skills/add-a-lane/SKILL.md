---
name: add-a-lane
description: Adds a new hot-path Lane to the engine (render, physics, audio, or UI). Use when implementing a new rendering technique, physics strategy, or any per-frame pipeline step that an Agent should be able to select.
---

# Add a Lane

A `Lane` is one behaviour of a family: the per-frame work, offered at one or more levels, which the
family's agent chooses between via GORNA. Trait in `crates/khora-core/src/lane/contract.rs`
(re-exported as `khora_core::lane::Lane`).

## Steps
1. Create `crates/khora-lanes/src/<domain>_lane/<name>.rs`. Implement `Lane`:
   `type Family` (the family whose outputs it writes), `type Inputs<'f>`, `levels` (cheapest first, each with
   a `StrategyId`, unique within the family), `supports` (optional), `prepare` (build what a level needs on
   the device, before the first record and when the level or the target changes), `cost` (price a level on
   this frame's inputs, per device), `record` (one frame's work at a level, returning a `LaneReport`) and
   `shutdown` (release every GPU resource `prepare` built).
2. **Hold only persistent state.** Put build/record bodies in private free functions in the module;
   the trait impl dispatches to them. Construct via `Default` (the registration builds lanes with it).
3. **Declare inputs as a struct deriving `LaneInputs`** — `#[bus]` the Flow's `View`, `#[deck]` an earlier
   phase's output, `#[resource]` / `#[locked]` runtime entries, `#[frame]`, `#[budget]`, `#[take]`, … — and
   **write the family's outputs** (`#[derive(LaneOutputs)]` on the family's `Outputs`). What these declare is
   the agent's contention. Never call `world.query*` from a lane.
4. If render: name the pipeline (`khora::pipelines::<name>`) in the lane's pipeline spec; respect
   the **4-bind-group** budget (see [`add-a-shader`](../add-a-shader/SKILL.md)).
5. Add the lane to its family's `register_agent!` registration (`lanes: [..]`,
   `crates/khora-agents/src/<domain>_agent/agent.rs`). A new family: see
   [`add-an-agent`](../add-an-agent/SKILL.md).
6. Add `#[cfg(test)]` unit tests; drive the family end to end without the scheduler through
   `khora_agents::agent::run_frame`.

## Rules
- Never bypass the `Lane` abstraction for hot-path work. Never expose raw GPU handles. No `unwrap()` on GPU/IO.
- An `Exact` family's levels must all have `Impact::None` (boot refuses a result-changing level).

## Verify
`cargo gate`; for render lanes, `cargo run -p sandbox` (clean frame). Load the matching domain reference
(`../../reference/graphics-rendering.md`, `physics.md`, `audio.md`).
