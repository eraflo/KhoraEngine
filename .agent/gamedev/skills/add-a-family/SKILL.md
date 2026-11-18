---
name: add-a-family
description: Adds a game's own engine subsystem — a family of lanes and its one register_agent! registration, run by the engine's LaneAgent — from the game's native crate. Use only when the work has several levels to negotiate under budget (AI, crowd simulation, a custom renderer pass); gameplay rules belong in Ergon.
---

# Add a family

A subsystem is a **family**: its lanes (the same work at several costs), a `LaneFamily` naming what
they write, and one `register_agent!`. The engine builds the family's agent (the one `LaneAgent`),
negotiates its budget through GORNA and runs the chosen lane as a layer every frame. **Never write an
agent type.** Walkthrough: the engine book's `docs/src/tutorials/extending-the-engine.md`; detail:
`docs/src/how-to/add-a-family.md` and `docs/src/how-to/add-a-lane.md`.

Is it a family? Only if it has levels to choose between. A gameplay rule is an Ergon `behavior` or
`system`; a resource the game reads is installed in `EngineApp::provide`.

## Steps
1. **Dependencies.** The SDK does not cover families yet: the lane derives name `khora_core`, and the
   registration types live in the engine crates. Add them beside `khora-sdk`, at the SDK's version:
   ```toml
   khora-core = "<engine_version>"
   khora-macros = "<engine_version>"
   khora-agents = "<engine_version>"
   ```
   (in the engine repo: `{ path = "crates/khora-core" }`, …).
2. **The family.** A unit struct implementing `khora_core::lane::family::LaneFamily`: `NAME`, `AGENT`
   (`AgentId::new("<name>")`, unique — boot refuses two families under one id), `SEMANTICS`
   (`Presentation` if a cheaper level may give a lesser result, `Exact` otherwise) and
   `type Outputs<'f>`, a struct deriving `khora_macros::LaneOutputs` (`#[deck]` a value later phases read).
3. **Its lanes.** Each implements `khora_core::lane::Lane` with `type Family = <YourFamily>`: `levels`
   (one `Level` per cost), `cost(level, inputs)`, `record(level, inputs, out)`. Inputs derive
   `LaneInputs`: `#[resource]` a runtime entry, `#[bus]` a View a `Flow` published. A lane never queries
   the world and never spawns a thread.
4. **The registration**, submitted once:
   ```rust
   use khora_agents::registered_agent::RegisteredAgent;
   use khora_core::agent::registration::AgentRegistration;
   use khora_core::agent::{AgentImportance, ExecutionPhase};

   pub const MY_FAMILY: AgentRegistration = khora_core::register_agent! {
       family: MyFamily,
       phase: ExecutionPhase::TRANSFORM,
       importance: AgentImportance::Optional,
       after: [],
       fixed_step: None,
       lanes: [MyLane],
   };

   khora_sdk::inventory::submit! {
       RegisteredAgent(MY_FAMILY)
   }
   ```
5. **Its resources.** Insert every `#[resource]` the lanes read in `EngineApp::provide`; boot refuses a
   family whose runtime entry is missing, naming it.
6. **Test it** without a window: `khora_agents::lane_agent::{lane_agent, run_frame}` run one frame and the
   lane's output is in the `OutputDeck` (the tutorial's Step 6 has the test).
7. **Run it.** `RUST_LOG=info cargo run`. Switch it with `Activation::{activate, deactivate}` and its
   `AgentId`.

## Rules
- One family, one `AgentId`, one `register_agent!`. No agent type, no `execute`, no `std::thread::spawn`.
- A lane reads Views from the `LaneBus` and writes its family's outputs — never the World.
- A family that reads entities needs a `Flow` publishing its View (`register_flow!`, engine book
  `docs/src/how-to/add-a-flow.md`); that brick lives in `khora-data`.
- Math via `khora_core::math`; log via `log::*`.
