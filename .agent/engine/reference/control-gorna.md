# Control / GORNA — reference

Domain knowledge for Khora's control plane. Consulted during Research (dispatch the `codebase-*`
subagents to apply it to concrete files). Follow [`../RULES.md`](../RULES.md) §3 and §5.

## Scope
Per-frame budget negotiation and arbitration, learned cost prediction, PID budget regulation, adaptation
modes, and tick ordering (Substrate Pass + agent descent).

## Key files
- DCC: `crates/khora-control/src/dcc_service/` (decision thread in `decision_loop.rs`); situational `Context` `crates/khora-control/src/dcc_context.rs`.
- GORNA: `crates/khora-control/src/gorna/` — `mod.rs` (`GornaArbitrator::allocate`: offers, calibration, lending; `reissue` for replay), `decide.rs` (the pure share: admission, Q-RAM upgrades, 80/20 slack, policies as envelope shapes). Epochs and triggers: `dcc_service/epochs.rs`. Protocol types: `crates/khora-core/src/agent/gorna/protocol.rs`.
- Cost model: `crates/khora-control/src/cost_model.rs` (learned linear `predict_ms`).
- PID: `crates/khora-control/src/pid.rs` (anti-windup, ~20 Hz cold path, output `[0.3, 1.0]`).
- Substrate dispatcher: `crates/khora-control/src/substrate/`.

## Concepts
- **GORNA is a protocol** (`OfferRequest` → `Offer` → `Allocation` → `Report`): per epoch, agents offer
  their rows (cost per device, quality from `Impact`, confidence, floor); the DCC admits the floors on the
  critical path, upgrades by weighed quality per µs, and lends each agent an **envelope** (time + memory);
  the agent chooses its row within it every frame. Replay lends recorded allocations.
- **Adaptation modes** are envelope shapes: `Learning` (free), `Manual(row)` (a pin), `Stable` (keeps its
  envelope while it fits), `Bounded(min, max)` (allowed rows); a `Cap` hint is a ceiling.
- **Only agents bid** for the budget; the Data layer (AGDF) self-optimizes and is only observed via telemetry.
- Memory-pressure / thermal / battery heuristics bias `AnalysisReport` toward efficient strategies.

## Hard rules
- The DCC lends envelopes and observes; it never chooses an agent's row (a developer pin aside) nor a data layout.
- No `std::thread::spawn` — concurrency flows through the DCC.

Use codegraph to trace `offer → allocate → decide → apply → execute`.

## Skills
- [`add-an-agent`](../skills/add-an-agent/SKILL.md) — add a budget-negotiating agent.
- [`debug-frame`](../skills/debug-frame/SKILL.md) — diagnose a wrong strategy pick / budget behaviour (uses replay).
- [`build-and-test`](../skills/build-and-test/SKILL.md) — verify.
