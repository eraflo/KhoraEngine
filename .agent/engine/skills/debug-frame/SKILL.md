---
name: debug-frame
description: Investigates a per-frame problem or a GORNA budget decision — wrong strategy selected, a stutter, a render glitch, or a negotiation that didn't behave. Use when diagnosing runtime frame behavior rather than a compile error.
---

# Debug a frame / GORNA decision

## Approach
1. **Reproduce** with `cargo run -p sandbox` (or the editor); capture the exact log lines and `RUST_LOG=debug`
   for the suspect subsystem.
2. **Locate** the path with codegraph: `codegraph_trace` from the agent's `execute` to the lane, or from
   `offer → allocate → decide → apply`.
3. **Inspect the descent**: which row did the agent choose, within which envelope (`Report.over_budget`)?
   The share is `decide` in `crates/khora-control/src/gorna/decide.rs` (admission, plan, share in its siblings); cost prediction in `cost_model.rs`;
   the multiplier in `crates/khora-control/src/governor/mod.rs`.
4. **Substrate vs descent**: confirm whether the issue is in Pass A (DataSystems/Flows publishing Views) or
   Pass B (agent → lane). A missing/stale `View` in the `LaneBus` is a common cause of "nothing renders".
5. **Replay**: GORNA replays recorded allocations (`DecisionTrace.epochs`, each an `EpochRecord` with its inputs) — use a recorded epoch to
   isolate negotiation from rendering.

## Known-issue checklist
- Scene not rendering → check bind-group count (must be 4) and that the Flow published its `View`.
- Camera jitter → matrix precision / shadow bias (see [`../../knowledge/MEMORY.md`](../../knowledge/MEMORY.md)).
- Vulkan semaphore warnings → known, usually non-fatal.

Find the root cause before patching; explain it concisely. As the cause narrows, consult
[`../../reference/control-gorna.md`](../../reference/control-gorna.md) or
[`../../reference/graphics-rendering.md`](../../reference/graphics-rendering.md).
