---
name: add-an-agent
description: Adds a new strategist Agent to the engine. Use only when a subsystem genuinely needs per-frame GORNA budget negotiation; otherwise add a direct service instead.
---

# Add an Agent

An `Agent` is a strategist: given a budget, it selects a `Lane` and reports status. Trait in
`crates/khora-core/src/agent/mod.rs`. **Most subsystems should NOT be agents** — only those that negotiate
a GORNA budget. Non-negotiating work uses a service (`AssetService`, `EcsMaintenance`, …).

## Steps
1. Add an `AgentId` variant (priority-ordered) in `crates/khora-core/src/agent/gorna.rs` / `agent/`.
2. Create `crates/khora-agents/src/<domain>_agent/{mod.rs,agent.rs}`. Implement **only** `Agent` + `Default`:
   `id`, `negotiate`, `apply_budget`, `report_status`, `on_initialize` (cache services once), `execute`
   (dispatch the lane), `as_any`/`as_any_mut`.
3. Declare `ExecutionTiming` (allowed phases, default phase, priority, importance, fixed timestep, dependencies),
   `access()` and `contention()` (RULES §5). Engine modes are **not** part of the timing: they are passed at
   registration (step 5).
4. In `negotiate`, return `NegotiationResponse` options (time, VRAM) per strategy; in `apply_budget`, pick the
   strategy the arbitrator granted; in `execute`, call `Lane::execute(LaneContext{bus, deck, budget})`.
5. Register the agent in `crates/khora-sdk/src/engine/bootstrap.rs` next to the existing agents:
   `dcc.register_agent(agent, priority)` (every mode) or
   `dcc.register_agent_for_mode(agent, priority, vec![EngineMode::Playing])` (only those modes — how
   `ScriptAgent` stays off while the editor edits). The scheduler reads `SharedEngineMode` once per frame.

## Hard rules
- **No method outside the `Agent` trait** — no `start/stop`, builders, or accessors. Private free functions
  in the module are fine.
- One agent per `LaneKind`; no per-frame state; no owning a Flow; no buffering outputs.

## Verify
`cargo gate`. For budget/negotiation design, consult [`../../reference/control-gorna.md`](../../reference/control-gorna.md).
