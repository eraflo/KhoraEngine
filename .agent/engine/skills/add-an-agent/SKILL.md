---
name: add-an-agent
description: Adds a new negotiating subsystem to the engine — a family of lanes and its one register_agent! registration, run by the engine's LaneAgent. Use only when a subsystem genuinely needs per-frame GORNA budget negotiation; otherwise add a direct service instead.
---

# Add an Agent (a family)

An agent is a strategist: it offers its family's rows to GORNA, is lent an envelope, chooses the row that
fits, and installs it as a layer at the family's place. Trait in `crates/khora-core/src/agent/mod.rs`
(`id`, `offer`, `apply`, `report`, `place`, `wake`, `execution_timing`, `contention`). **Most subsystems
should NOT be agents** — only those that negotiate a GORNA budget. Non-negotiating work uses a service
(`AssetService`, `EcsMaintenance`, …).

> **A new subsystem is a family, never a new agent type** (RULES §8): its lanes, a `LaneFamily`, and one
> `register_agent!` registration. Its agent is always the one `LaneAgent`
> (`crates/khora-agents/src/agent/mod.rs`), built from the registration. `LaneAgent` is the only
> implementor of `Agent` outside tests (xtask `only_lane_agent_implements_agent`).

## Steps
1. **Write the family** in `crates/khora-lanes/src/<domain>_lane/<domain>_family.rs`: a unit struct
   implementing `khora_core::lane::family::LaneFamily` (`crates/khora-core/src/lane/family.rs`) — `NAME`, `AGENT`
   (`AgentId::new("<name>")`, or a core constant such as `khora_core::audio::AUDIO`), `SEMANTICS`
   (`Presentation`, or `Exact` when every level must give the same result), and `type Outputs<'f>`, a struct
   deriving `LaneOutputs` (`#[pass]`, `#[deck]`, `#[staged]`). Boot refuses two families naming one agent.
   Priority comes from the declared `AgentImportance`, never from the name. Model: `audio_lane/audio_family.rs`.
2. **Write its lanes** — see [`add-a-lane`](../add-a-lane/SKILL.md). Each names `type Family = <YourFamily>`;
   a lane of another family does not compile into the registration.
3. **Register it** in `crates/khora-agents/src/<domain>_agent/agent.rs`:
   ```rust
   khora_script::inventory::submit! {
       RegisteredAgent(khora_core::register_agent! {
           family: MyFamily,
           phase: ExecutionPhase::TRANSFORM,
           importance: AgentImportance::Important,
           after: [],
           fixed_step: None,          // Some(step) for a fixed-step family; the step lives at its place
           // default_level: StrategyId::Balanced,   (optional)
           lanes: [MyLane],
       })
   }
   ```
   Boot collects every registration (`registered_agents()`), checks them together
   (`check_registrations`, `crates/khora-core/src/agent/registration.rs`) and builds one `LaneAgent` each.
   The family starts on; the app switches it with `activate`/`deactivate` on the `Arc<Activation>`
   resource, settled at the next frame boundary (how the script family is off while the editor edits). A
   DataSystem or Flow doing its work declares `Role::Serves(<its AgentId>)`; one keeping a data invariant
   declares `Role::Upkeep`.
4. **Nothing else.** The agent's contention is built from its lanes' typed inputs and outputs; the
   scheduler runs the installed layer every frame and wakes the agent only for an event. An app never
   registers an agent (`EngineApp::provide` installs runtime entries only).
5. **Test the family without the scheduler** through `khora_agents::agent::run_frame` (build the agent with
   `lane_agent(registration, &caps)`, the registration found in `registered_agents()`); integration tests go in `crates/khora-agents/tests/it/` (one module per
   family, e.g. `audio_family.rs`).

## Hard rules
- Never write a new agent type, and never give an agent more than one family.
- Never offer one `StrategyId` twice in a family — boot refuses it.
- No agent reaches the `World`: lanes read Views from the bus and write the deck.
- No per-frame state on the agent; no owning a Flow; no buffering outputs.

## Verify
`cargo gate`. For budget/negotiation design, consult [`../../reference/control-gorna.md`](../../reference/control-gorna.md).
