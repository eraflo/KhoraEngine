---
name: add-an-agent
description: Adds a new strategist Agent to the engine. Use only when a subsystem genuinely needs per-frame GORNA budget negotiation; otherwise add a direct service instead.
---

# Add an Agent

An `Agent` is a strategist: given a budget, it selects a `Lane` and reports status. Trait in
`crates/khora-core/src/agent/mod.rs`. **Most subsystems should NOT be agents** — only those that negotiate
a GORNA budget. Non-negotiating work uses a service (`AssetService`, `EcsMaintenance`, …).

## Steps
1. Add an `AgentId` variant (priority-ordered) in `crates/khora-core/src/control/gorna/` / `agent/`.
2. Create `crates/khora-agents/src/<domain>_agent/{mod.rs,agent.rs}`. Implement **only** `Agent` + `Default`:
   `id`, `negotiate`, `apply_budget`, `report_status`, `on_initialize` (cache services once), `execute`
   (dispatch the lane), `as_any`/`as_any_mut`.
3. Declare `ExecutionTiming` (allowed phases, priority, importance, dependencies) and the allowed `EngineMode`.
4. In `negotiate`, return `NegotiationResponse` options (time, VRAM) per strategy; in `apply_budget`, pick the
   strategy the arbitrator granted; in `execute`, call `Lane::execute(LaneContext{bus, deck, budget})`.
5. Register the agent in `crates/khora-sdk/src/engine.rs` next to the existing agents.

## Hard rules
- **No method outside the `Agent` trait** — no `start/stop`, builders, or accessors. Private free functions
  in the module are fine.
- One agent per `LaneKind`; no per-frame state; no owning a Flow; no buffering outputs.

## Verify
`cargo test --workspace`. For budget/negotiation design, consult [`../../reference/control-gorna.md`](../../reference/control-gorna.md).
