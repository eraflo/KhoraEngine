# Control / GORNA — reference

Domain knowledge for Khora's control plane. Consulted during Research (dispatch the `codebase-*`
subagents to apply it to concrete files). Follow [`../RULES.md`](../RULES.md) §3 and §5. Book:
`docs/src/concepts/gorna.md`, `the-frame.md`, `agents-and-lanes.md`.

## Scope
The DCC, GORNA as a protocol (offers, allocations, reports), the arbiter, the cost book and the frame
governor, adaptation modes, and the scheduler that lends envelopes and runs each family's layer.

## Key files
- Protocol types: `crates/khora-core/src/agent/gorna/` — `offer_request.rs`, `offer.rs`, `protocol.rs`
  (`Allocation`, `Envelope`, `Epoch`, `Trigger`, `Why`), `report.rs`, `frame_report.rs`, `decision_trace.rs`.
- DCC: `crates/khora-control/src/dcc_service/` (the decision thread `decision_loop.rs`, epochs and triggers
  `epochs.rs`); situational `DccContext` `crates/khora-control/src/dcc_context.rs`.
- GORNA round: `crates/khora-control/src/gorna/` — `mod.rs` (`GornaArbitrator::allocate`, `reissue` for
  replay), `decide.rs`, `admission.rs` (the floors), `plan.rs`, `share.rs` (the slack, policies as envelope
  shapes), `units.rs`, `in_quotes.rs`, `cost_book.rs` (learnt corrections, profiles across runs).
- Arbiter: `crates/khora-control/src/gorna/arbiter/` — `Proposer`s (`greedy.rs` the Q-RAM pass, `exact.rs`
  a branch-and-bound under a node budget), `shield.rs` (refuses a plan that breaks a frame constraint or a
  policy), `mod.rs` (`Decision`: the best plan the shield kept).
- Frame governor: `crates/khora-control/src/governor/mod.rs`; the frame time is the host's `FrameReport`,
  measured by `khora-sdk`'s frame clock.
- Scheduler: `crates/khora-control/src/scheduler/` — `lending.rs` (`lend_allocation` at the frame start),
  `layers.rs` (`swap_layers`, `wake_agents`), `known.rs` (each agent's place cached: a quiet frame locks no
  agent), `execution.rs` + `waves.rs` (waves split on contention), `background.rs` (the `BACKGROUND` pool).
- Places and layers: `crates/khora-core/src/agent/layer/` (`Place`, `Layer`, `Wake`, `KEEP_EPOCHS`);
  `LaneAgent`'s side `crates/khora-agents/src/lane_agent/{choice,quotes,layer,wake}.rs`.

## Concepts
- **GORNA is a protocol** (`OfferRequest` → `Offer` → `Allocation` → `Report`): per epoch, each agent
  offers its rows (cost per device, quality from `Impact`, confidence, floor); the DCC admits the floors on
  the critical path, the arbiter plans the upgrades, and each agent is lent an **envelope** (time per device
  + memory). The agent chooses its row within it; replay lends recorded allocations.
- **Lanes are layers.** A family's `Place` holds the installed layer, the staged one and those kept. Frame
  order: lend → `swap_layers` (each place's `start_frame`) → after the bus, `wake_agents` → waves run each
  place's installed layer (`Place::run`). A family switched back on is `withdraw`n and lent only
  allocations taken after.
- **Adaptation modes** are envelope shapes: `Learning` (free), `Manual(row)` (a pin), `Stable` (keeps its
  envelope while it fits), `Bounded(min, max)` (allowed rows); a `Cap` hint is a ceiling.
- **Only agents bid**; the Data layer (AGDF) self-optimises and is only observed.

## Hard rules
- The DCC lends envelopes and observes; it never chooses an agent's row (a developer pin aside) nor a data layout.
- A new planning strategy is a `Proposer` behind the `Shield`, never a path around it.
- A swap happens only at a frame start, at the family's place; no frame runs without a layer. Lock order:
  an agent's lock, then its place's — never the reverse.
- A panic in a run, a wake or a frame start fails that family alone; its wakes are handed back.
- No `std::thread::spawn` — concurrency flows through the DCC (RULES §5).

Use codegraph to trace `offer → allocate → decide → publish → lend_allocation → apply → wake_agents → Place::run → Report`.

## Skills
- [`add-a-family`](../skills/add-a-family/SKILL.md) — add a budget-negotiating subsystem.
- [`debug-frame`](../skills/debug-frame/SKILL.md) — diagnose a wrong row pick / budget behaviour (uses replay).
- [`build-and-test`](../skills/build-and-test/SKILL.md) — verify.
