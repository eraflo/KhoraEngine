---
name: add-a-lane
description: Adds a new hot-path Lane to the engine (render, physics, audio, or UI). Use when implementing a new rendering technique, physics strategy, or any per-frame pipeline step a family's agent should be able to choose.
---

# Add a Lane

A `Lane` is one behaviour of a family: the per-frame work, offered at one or more levels, among which the
family's `LaneAgent` chooses the row that fits its GORNA envelope. The chosen lane runs as the family's
installed **layer**, every frame, without the agent. Trait: `crates/khora-core/src/lane/contract/lane.rs`
(`khora_core::lane::Lane`). Book: `docs/src/how-to/add-a-lane.md`.

## Steps
1. **The file.** `crates/khora-lanes/src/<family>/<lane>.rs` (a folder `<lane>/` when it holds several
   files, as `render/forward_plus/`); declare it in the family's `mod.rs`. A new family first:
   [`add-a-family`](../add-a-family/SKILL.md). Model: `crates/khora-lanes/src/audio/spatial_mixing.rs`.
2. **Its inputs** as a struct deriving `LaneInputs` (`khora_macros::LaneInputs`); each field names its
   source: `#[bus]` a View (`&'f V`; `#[bus(validate)]` runs its `Validate` check), `#[frame]` the frame's
   target (`Target`, `SceneTarget`) or `ClearColor`, `#[deck]` a slot an earlier phase wrote (`Option<&'f T>`), `#[resident]` an
   asset's device copy (`&'f Resident<R>`), `#[resource]` a runtime entry read without locking,
   `#[locked]` one locked for the record (`Locked<'f, T>`), `#[take]` the family's `Take` mailbox,
   `#[budget]` the time lent (`Option<Budget>`). These are the family's contention: declare only what the
   lane reads. Never reach the `World`.
3. **Implement `Lane`:** `type Family`, `type Inputs<'f>`, `levels` (cheapest first, each a `Level` with a
   `StrategyId` unique in the family and its `Impact`), `supports` (optional: a lane the device cannot run
   leaves the menu at boot), `prepare` (build device resources for a level and a `TargetKey`; called again
   when either changes), `cost` (price a level on this frame's inputs, per device), `record` (one frame at a
   level, writing the family's `Outputs`, returning a `LaneReport`) and `shutdown` (release what `prepare`
   built). Hold only state that persists across frames; `record` takes `&mut self`. The registration
   builds the lane with `Default`.
4. **Render lanes:** name the pipeline in its spec (`shader: "khora::pipelines::<name>"`), build it per
   `TargetKey`, keep the **4-bind-group** budget (see [`add-a-shader`](../add-a-shader/SKILL.md)), and
   write GPU work to a `#[pass]` output of the family.
5. **Register it:** add it to `lanes: [..]` of the family's `register_agent!`
   (`crates/khora-lanes/src/<family>/registration.rs`).
6. **Tests:** a `#[cfg(test)]` unit test per public fn; the lane's integration tests in
   `crates/khora-lanes/tests/it/<family>/`; the family end to end in
   `crates/khora-agents/tests/it/families/<family>/` through `khora_agents::lane_agent::run_frame`.

## Rules
- Never bypass the `Lane` abstraction for hot-path work. Never expose raw GPU handles. No `unwrap()` on GPU/IO.
- An `Exact` family's levels all have `Impact::None` (boot refuses a result-changing level).
- A `LaneError` from `record` falls back to a cheaper row; return one rather than panicking.

## Verify
`cargo xtask lint-file <every .rs touched>`; `cargo t -p khora-lanes` and `cargo t -p khora-agents`;
`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items`; then `cargo gate`.
For a render lane, `cargo run -p sandbox` (clean frame, no validation error). Domain reference:
`../../reference/graphics-rendering.md`, `physics.md`, `audio.md`.
