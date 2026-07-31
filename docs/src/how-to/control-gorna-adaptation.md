# Control GORNA adaptation

This guide shows you how to **pin or bound an agent's adaptation** from game or host
code, so GORNA's automatic strategy switching stays within limits you choose — and how
to record and replay the decisions it makes.

**Prerequisites:** you have read [GORNA](../concepts/gorna.md).

> This is the developer-control surface over the adaptive core: the DCC observes and
> proposes, but you decide how much latitude it has. A safety stop can still force the
> lowest strategy in any mode — safety overrides developer control.

## Step 1 — Choose an `AdaptationMode`

`AdaptationMode` sets how much freedom GORNA has for one agent:

| Mode | Behaviour |
|---|---|
| `Learning` (default) | Full negotiation — GORNA freely picks the best-fitting strategy each tick. |
| `Manual(StrategyId)` | **Pinned** — the agent stays on the given strategy; GORNA observes but never switches it. |
| `Stable` | **Predictable** — GORNA may *downgrade* under budget pressure but never makes an opportunistic *upgrade*, so the strategy doesn't flap. |
| `Bounded { min, max }` | **Learning within limits** — the chosen strategy is clamped to `[min, max]` (`LowPower < Balanced < HighPerformance`; `Custom` ranks above `HighPerformance`). |

## Step 2 — Apply it through `DccService::set_adaptation_mode`

Call `set_adaptation_mode` on the `DccService` with the target `AgentId` and the mode.
It is thread-safe and takes effect on the next arbitration tick.

```rust
use khora_core::control::gorna::{AdaptationMode, AgentId, StrategyId};

// Pin the renderer to LowPower (e.g. on battery): GORNA will not upgrade it.
dcc.set_adaptation_mode(AgentId::Renderer, AdaptationMode::Manual(StrategyId::LowPower));

// Or let physics learn, but never below Balanced and never above HighPerformance:
dcc.set_adaptation_mode(
    AgentId::Physics,
    AdaptationMode::Bounded {
        min: StrategyId::Balanced,
        max: StrategyId::HighPerformance,
    },
);

// Or stop quality from flapping for the UI agent:
dcc.set_adaptation_mode(AgentId::Ui, AdaptationMode::Stable);
```

To return an agent to full autonomy, set it back to `AdaptationMode::Learning`.

## Step 3 — (Optional) record and replay decisions

GORNA arbitration is deterministic (no RNG), so recording the strategy issued to each
agent each tick and replaying it reproduces a session's adaptation **bit-for-bit** —
useful for QA, network lockstep, and bug reproduction.

```rust
// Start capturing per-tick decisions into a fresh trace.
dcc.start_decision_recording();

// … run the engine …

// Stop and take the captured trace (a `DecisionTrace`).
let trace = dcc.stop_decision_recording();

// Later: replay it. Each tick re-issues the recorded strategies in order,
// bypassing live fit and AdaptationMode, until the trace is exhausted.
dcc.replay_decisions(trace);

// Return to live arbitration at any point.
dcc.stop_replay();
```

`recorded_decisions()` returns a snapshot of the trace captured so far without stopping
recording.

## Step 4 — Verify it works

```bash
cargo test --workspace
```

With recording on, drive a few ticks, replay the captured trace, and assert the issued
strategies match the original run. To confirm a mode took effect, read the agent's
reported `current_strategy` and check it respects the bound you set.

## Related

- [GORNA](../concepts/gorna.md) — the negotiation protocol and strategy fitting.
- [Add an agent](./add-an-agent.md) — the agent whose adaptation you are controlling.
