# Control GORNA adaptation

This guide shows you how to **pin, bound or steady an agent's adaptation** from host
code, how to weigh or cap an agent with a hint, and how to record and replay the
decisions GORNA makes.

**Prerequisites:** you have read [GORNA](../concepts/gorna.md). The words *row*,
*floor* and *envelope* are defined in [the protocol](../concepts/gorna-protocol.md).

> This is the developer-control surface over the adaptive core: the DCC decides how
> much each agent gets, and you decide how much latitude it has. The `Safety`
> allocation still gives every agent its floor in any mode: safety overrides developer
> control.

## Step 1 — Find the agent's name

Every family names its agent (`LaneFamily::AGENT`): `renderer`, `shadows`, `ui`,
`skybox`, … An `AgentId` is that name: `AgentId::new("shadows")`. The editor's
Control Plane lists every agent registered, with the row it runs.

## Step 2 — Choose an `AdaptationMode`

| Mode | Behaviour |
|---|---|
| `Learning` (default) | Full negotiation: the DCC lends an envelope, the agent picks its best row within it. |
| `Manual(StrategyId)` | **Pinned**: the round plans that row, and the agent runs it whatever its envelope. |
| `Stable` | **Steady**: the agent keeps the envelope it holds while that still fits the frame with every other agent at its floor, and takes no upgrade. One that no longer fits becomes a ceiling: it can still step down, never up past it. |
| `Bounded { min, max }` | **Within limits**: the rows planned and chosen are those in `min..=max`, ordered `LowPower < Balanced < HighPerformance < Custom(n)` by *n*. |

A pin or a range that names no row of the agent's menu limits nothing.

## Step 3 — Apply it through `DccService::set_adaptation_mode`

The host that owns the engine reaches its DCC through `EngineCore::dcc()`. The call is
thread-safe; the next round reads it.

```rust
use khora_sdk::khora_core::agent::gorna::AdaptationMode;
use khora_sdk::{AgentId, StrategyId};

if let Some(dcc) = core.dcc() {
    // Pin the shadows to their cheapest level, on battery for instance.
    dcc.set_adaptation_mode(AgentId::new("shadows"), AdaptationMode::Manual(StrategyId::LowPower));

    // Let the renderer learn, but never below Balanced.
    dcc.set_adaptation_mode(
        AgentId::new("renderer"),
        AdaptationMode::Bounded {
            min: StrategyId::Balanced,
            max: StrategyId::HighPerformance,
        },
    );

    // Keep the UI from flapping between rows.
    dcc.set_adaptation_mode(AgentId::new("ui"), AdaptationMode::Stable);
}
```

To return an agent to full autonomy, set it back to `AdaptationMode::Learning`.

A mode does not open a round by itself: it takes effect at the next round, whatever
opens it — a heuristic, the frame governor, a family switched on or off. On a quiet,
steady machine that can take a while; the
[triggers](../concepts/gorna-protocol.md#epochs-and-triggers) list what opens one.

## Step 4 — (Optional) weigh or cap an agent with a hint

A hint biases the share without pinning anything (`EngineHint`):

```rust
use khora_sdk::{AgentId, EngineHint};

// Count the UI more than its importance says.
core.set_engine_hint(EngineHint::Prioritize { agent: AgentId::new("ui"), weight: 1.0 });

// Never lend the shadows more than 2 ms a frame (never less than their floor).
core.set_engine_hint(EngineHint::Cap { agent: AgentId::new("shadows"), max_ms: 2.0 });

// Back to the engine's defaults.
core.clear_agent_hints(AgentId::new("shadows"));
```

`Prioritize` replaces the weight the agent's importance gives it (Critical 1.0,
Important 0.75, Optional 0.5). `Cap` rules out every row above it and bounds the
envelope's time; send a large `max_ms` to lift a cap. The latest hint of each kind
wins; a NaN, or a weight that is not finite, is refused and logged.

## Step 5 — (Optional) record and replay decisions

A round is a pure function of what it read, so recording each epoch with its inputs
and replaying the allocations reproduces a session's adaptation exactly — for QA,
lockstep and bug reproduction.

```rust
// Start journaling every epoch into a fresh trace.
dcc.start_decision_recording();

// … run the engine …

// Stop and take the journal (a `DecisionTrace`).
let trace = dcc.stop_decision_recording();

// Later: lend its allocations again, one per DCC tick, each at a frame start
// of its own, asking no agent for an offer. Live rounds resume once it runs out.
dcc.replay_decisions(trace);

// Return to live rounds at any point.
dcc.stop_replay();
```

`recorded_decisions()` returns the journal so far without stopping. To record a whole
run without code, start any app with `KHORA_RECORD_DECISIONS=<file>`: the engine
records from its first round and writes the journal there as JSON at shutdown. To
compare the arbiter's proposer sets on that journal:

```text
cargo run --release -p khora-sdk --example arbiter_eval journal.json
```

## Step 6 — Verify it works

- **In the editor,** select the agent in the Control Plane: its decision card shows
  the row planned and the row running. A pinned agent runs its pin; a bounded one
  stays in its range.
- **In code,** read the agent's last `Report` (`Agent::report`): its `row` must
  respect the mode you set.
- **For a replay,** compare the allocations lent with the journal's
  `EpochRecord.allocation`, epoch by epoch.

## Related

- [GORNA](../concepts/gorna.md) — the modes and hints in context.
- [The arbiter](../concepts/arbiter.md) — how policies shape the plan, and the journal.
- [Debug a frame](./debug-a-frame.md) — read why a decision was made.
- [Add a subsystem (a family)](./add-a-family.md) — the agent whose adaptation you are
  controlling.
