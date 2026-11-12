# Control / GORNA — reference

Domain knowledge for Khora's control plane. Consulted during Research (dispatch the `codebase-*`
subagents to apply it to concrete files). Follow [`../RULES.md`](../RULES.md) §3 and §5.

## Scope
Per-frame budget negotiation and arbitration, learned cost prediction, frame-governor budget regulation, adaptation
modes, and tick ordering (Substrate Pass + agent descent).

## Key files
- DCC: `crates/khora-control/src/dcc_service/` (decision thread in `decision_loop.rs`); situational `Context` `crates/khora-control/src/dcc_context.rs`.
- GORNA: `crates/khora-control/src/gorna/` — `mod.rs` (`GornaArbitrator::allocate`: offers, calibration, the round; `reissue` for replay; it lends nothing — the scheduler lends the published allocation at its frame start, `scheduler/frame.rs` `lend_allocation`), channel `budget_channel.rs`, journal `khora-core/src/agent/gorna/decision_trace.rs`, `decide.rs` (the pure share: admission, Q-RAM upgrades, 80/20 slack, policies as envelope shapes). Epochs and triggers: `dcc_service/epochs.rs`. Protocol types: `crates/khora-core/src/agent/gorna/protocol.rs`.
- Cost model: `crates/khora-control/src/cost_model.rs` (learned linear `predict_ms`).
- Frame governor: `crates/khora-control/src/governor/mod.rs` (ratio law on the frame reports' critical path, 10 % headroom, band + overshoot memory, hitch rejection, no lowering when every agent is on its floor; output `[0.3, 1.0]`). The frame time comes from `FrameReport`, the host's report (`khora-core/src/agent/gorna/protocol.rs`), measured by `khora-sdk`'s frame clock.
- Substrate dispatcher: `crates/khora-control/src/substrate/`.

## Concepts
- **GORNA is a protocol** (`OfferRequest` → `Offer` → `Allocation` → `Report`): per epoch, agents offer
  their rows (cost per device, quality from `Impact`, confidence, floor); the DCC admits the floors on the
  critical path, upgrades by weighed quality per µs, and lends each agent an **envelope** (time + memory);
  the agent chooses its row within it every frame. Replay lends recorded allocations.
- **Lanes are layers** (`khora-core/src/agent/layer/`): a family's `Place` holds the installed `Layer`, the staged one, the ones kept (`KEEP_EPOCHS` = 3) and the `Wake`s. Frame order (`scheduler/frame.rs`): lend → `swap_layers` (each place's `start_frame`) → after the bus, `wake_layered` (agent lock, then place lock) → waves run `place.run` (`execution.rs` `run_once`), contending as the installed layer. The scheduler caches each layered agent (`scheduler/known.rs`): a quiet frame locks none. `LaneAgent`'s side: `khora-agents/src/lane_agent/{layer,wake}.rs`.
- **Adaptation modes** are envelope shapes: `Learning` (free), `Manual(row)` (a pin), `Stable` (keeps its
  envelope while it fits), `Bounded(min, max)` (allowed rows); a `Cap` hint is a ceiling.
- **Only agents bid** for the budget; the Data layer (AGDF) self-optimizes and is only observed via telemetry.
- Memory-pressure / thermal / battery heuristics bias `AnalysisReport` toward efficient strategies.

## Hard rules
- The DCC lends envelopes and observes; it never chooses an agent's row (a developer pin aside) nor a data layout.
- A swap happens only at a frame start, at the family's place; no frame runs without a layer. Lock order: an agent's lock, then its place's — never the reverse.
- A panic in a run, a wake or a frame start fails that family alone; its wakes are handed back.
- No `std::thread::spawn` — concurrency flows through the DCC.

Use codegraph to trace `offer → allocate → decide → publish → lend_allocation → apply → wake_layered → LaneAgent::answer → Place::run → AgentReport`.

## Skills
- [`add-an-agent`](../skills/add-an-agent/SKILL.md) — add a budget-negotiating agent.
- [`debug-frame`](../skills/debug-frame/SKILL.md) — diagnose a wrong strategy pick / budget behaviour (uses replay).
- [`build-and-test`](../skills/build-and-test/SKILL.md) — verify.
