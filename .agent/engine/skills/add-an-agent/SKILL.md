---
name: add-an-agent
description: Adds a new strategist Agent to the engine. Use only when a subsystem genuinely needs per-frame GORNA budget negotiation; otherwise add a direct service instead.
---

# Add an Agent

An `Agent` is a strategist: given a budget, it selects a `Lane` and reports status. Trait in
`crates/khora-core/src/agent/mod.rs`. **Most subsystems should NOT be agents** — only those that negotiate
a GORNA budget. Non-negotiating work uses a service (`AssetService`, `EcsMaintenance`, …).

> **A new subsystem is a family, not an agent** (RULES §8): its lanes and one `register_agent!`
> registration (phase, importance, lanes, default level); its agent is the one `LaneAgent`, which runs the
> lanes as layers. The steps below write a hand-written agent — what only `RenderAgent` and `ScriptAgent`
> still are.

## Steps
1. Name the agent: `AgentId::new("<name>")` — for a lane family, its `LaneFamily::AGENT` constant. The core
   lists no agent (`AgentId` is a name, `crates/khora-core/src/agent/gorna/mod.rs`); boot refuses a duplicate
   name. Priority comes from the declared `AgentImportance`, never from the name.
2. Create `crates/khora-agents/src/<domain>_agent/{mod.rs,agent.rs}`. Implement **only** `Agent` + `Default`:
   `id`, `negotiate`, `apply_budget`, `report_status`, `on_initialize` (initialise the lanes), `execute`
   (dispatch the lane), `as_any`/`as_any_mut`.
3. Declare `ExecutionTiming` (allowed phases, default phase, priority, importance, fixed timestep, dependencies),
   `access()` and `contention()` (RULES §5). Whether the agent runs is **not** part of the timing: the
   `Activation` switches it at run time (step 5).
4. In `negotiate`, return `NegotiationResponse` options (time, VRAM) per strategy; in `apply_budget`, pick the
   strategy the arbitrator granted; in `execute`, look services up through `context.resource` / `context.locked`
   (never `context.runtime`), put what the lane needs into a fresh `LaneContext` (a type map: `insert`,
   `insert_ref`, `insert_slot`) and call `Lane::execute(&mut ctx)`.
5. Register the agent. An engine agent goes in `crates/khora-sdk/src/engine/bootstrap.rs` next to the
   existing agents; a game or host agent goes through `AgentProvider::register_agents` (see
   `docs/src/how-to/add-an-agent.md`). Both call:
   `dcc.register_agent(agent, priority)`. The agent is a family and starts on; the app switches it with
   `activate`/`deactivate` on the `Arc<Activation>` resource, settled at the next frame boundary (how
   `ScriptAgent` is off while the editor edits). A DataSystem or Flow doing its work declares
   `Role::Serves(<its AgentId>)`; one keeping a data invariant declares `Role::Upkeep`.

## Hard rules
- **No method outside the `Agent` trait** — no `start/stop`, builders, or accessors. Private free functions
  in the module are fine.
- One agent per `LaneKind`; no per-frame state; no owning a Flow; no buffering outputs.

## Verify
`cargo gate`. For budget/negotiation design, consult [`../../reference/control-gorna.md`](../../reference/control-gorna.md).
