---
name: add-a-family
description: Adds a new negotiating subsystem to the engine — a family of lanes and its one register_agent! registration, run by the engine's LaneAgent. Use only when a subsystem genuinely needs per-frame GORNA budget negotiation; otherwise add a direct service instead.
---

# Add a family

A domain writes **a family, lanes and a registry line; `LaneAgent` is the only `Agent`** (RULES §8). The
family names the outputs its lanes share; its lanes are the behaviours GORNA chooses between; its registry
line tells boot where and when it runs. Boot builds one `LaneAgent` from it
(`crates/khora-agents/src/lane_agent/`), which offers the rows, is lent an envelope, chooses the row that fits
and installs it as a layer at the family's place. **Most subsystems are not families** — work with no
quality levels to negotiate is a service (`AssetService`, `EcsMaintenance`, …). Book:
`docs/src/concepts/agents-and-lanes.md`.

## Steps
1. **The folder.** `crates/khora-lanes/src/<family>/` holding `mod.rs`, `family.rs`, one file per lane and
   `registration.rs`; add `pub mod <family>;` to `crates/khora-lanes/src/lib.rs` and
   `("<family>", "<family>")` to `FAMILY_FOLDERS` in `xtask/tests/layout.rs` (the family folder and
   family test checks read it). Model: `crates/khora-lanes/src/audio/`.
2. **The family** (`family.rs`): a unit struct implementing `khora_core::lane::family::LaneFamily`
   (`crates/khora-core/src/lane/contract/family.rs`) — `NAME`, `AGENT` (`AgentId::new("<name>")`, or a
   core constant such as `khora_core::audio::AUDIO` when other crates name the family), `SEMANTICS`
   (`Presentation`, or `Exact` when every level must give the same result), and `type Outputs<'f>`, a
   struct deriving `LaneOutputs` (`#[pass]` GPU commands for a pass slot, `#[deck]` a value a later phase
   reads, `#[staged]` results held in the reserve until a frame boundary).
3. **Its lanes** — see [`add-a-lane`](../add-a-lane/SKILL.md). Each names `type Family = <YourFamily>`;
   a lane of another family does not compile into the registration.
4. **The registry line** (`registration.rs`), the folder's only `register_agent!`:
   ```rust
   use super::{MyFamily, MyLane};
   use khora_core::agent::{AgentImportance, ExecutionPhase, RegisteredAgent};

   khora_script::inventory::submit! {
       RegisteredAgent(khora_core::register_agent! {
           family: MyFamily,
           phase: ExecutionPhase::TRANSFORM,
           importance: AgentImportance::Important,
           after: [],
           fixed_step: None,              // Some(step) for a fixed-step family; the step lives at its place
           // default_level: StrategyId::Balanced,   (optional; else the menu's first row)
           lanes: [MyLane],
       })
   }
   ```
   Priority comes from the importance, never from the name. Boot collects every registration
   (`registered_agents()`), checks them together (`check_registrations`,
   `crates/khora-core/src/agent/registration.rs`) and refuses the set with a `BootError`: two families
   for one `AgentId`, a level offered twice, an `Exact` family with a result-changing level, a deck read
   with no earlier writer, an unknown runtime entry, a `BACKGROUND` family reading frame inputs, …
5. **The data side, each in its brick** (RULES §3): a `Flow` for the View the lanes read
   (`register_flow!`); a `DataSystem` for what writes the World back from the deck (`Maintenance`); a
   residency for an asset's copy on a device (`register_residency!`); a pass slot for GPU work
   (`register_pass!`). A DataSystem or Flow doing the family's work declares `Role::Serves(<AgentId>)`, so
   it is skipped while the family is off; one keeping a data invariant declares `Role::Upkeep`.
6. **Nothing else.** No agent type, no contention, no wiring: the contention is derived from the lanes'
   typed I/O, the family starts on (an app switches it through `Activation`), and an app never registers
   an agent (`EngineApp::provide` installs runtime entries only).
7. **Tests.** The lanes' unit tests beside them; the lanes' integration tests in
   `crates/khora-lanes/tests/it/<family>/`; the family end to end in
   `crates/khora-agents/tests/it/families/<family>/` — find the registration in `registered_agents()` by
   `TypeId::of::<MyFamily>()`, build the agent with `lane_agent(registration, &DeviceCaps::default())`
   and drive frames with `khora_agents::lane_agent::run_frame` (model: `families/audio/mod.rs`).

## Hard rules
- Never write an `Agent` impl (xtask `only_lane_agent_implements_agent`), and never give an agent more than one family.
- Never offer one `StrategyId` twice in a family.
- No agent or lane reaches the `World`: lanes read Views from the bus and write the family's outputs.

## Verify
`cargo xtask lint-file <every .rs touched>`; `cargo t -p khora-lanes`, `cargo t -p khora-agents`,
`cargo t -p xtask` (`every_family_folder_holds_its_family_and_registration`,
`registrations_live_in_their_family_folder`, `family_tests_live_in_their_family_folder`);
`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items`; then `cargo gate`.
For the negotiation, consult [`../../reference/control-gorna.md`](../../reference/control-gorna.md).
