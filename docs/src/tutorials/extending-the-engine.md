# Extending the engine

So far you have written *game* logic. This lesson crosses into *engine* work: you
will add your own subsystem — a custom **agent** that runs every frame and a
**lane** that does its work — register it, and watch the scheduler run it. By the
end the engine's frame loop will dispatch your code and you will see it log each
frame.

The example is deliberately tiny: a "heartbeat" subsystem that logs a message at
a steady cadence. The mechanics are exactly the ones a real subsystem (AI,
scripting, networking) uses.

Start from a working Khora app — the project from
[Your first game](./your-first-game.md) is fine.

## What you are building

Two pieces, in the order the engine uses them:

- A **lane** — the executor. It implements the [`Lane`] trait and contains the
  actual work. Lanes are swappable strategies; an agent picks one per frame.
- An **agent** — the strategist. It implements the [`Agent`] trait, owns one
  `LaneKind`, negotiates a budget through GORNA, and dispatches its lane.

> Why two pieces? The agent *decides*, the lane *does*. This split is what lets the
> engine swap quality strategies under load. See
> [Agents and Lanes](../concepts/agents-and-lanes.md).

## Step 1 — Define the lane

Create `src/heartbeat.rs`. The lane implements `Lane`: a name, a `LaneKind`, and
an `execute` that does the work. Note `execute` takes `&self` and returns
`Result<(), LaneError>`.

```rust
use khora_sdk::khora_core::lane::{Lane, LaneContext, LaneError, LaneKind};

#[derive(Default)]
pub struct HeartbeatLane;

impl Lane for HeartbeatLane {
    fn strategy_name(&self) -> &'static str {
        "Heartbeat"
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Ecs
    }

    fn execute(&self, _ctx: &mut LaneContext) -> Result<(), LaneError> {
        log::info!("heartbeat: tick");
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
```

A real lane reads its inputs from the **`LaneContext`** (`ctx.get::<T>()`) — for
hot-path work, that data comes from **Views the agent reads off the `LaneBus`**,
never by querying the world directly. Our heartbeat needs no input, so it just
logs.

> Why read Views, not the world? It keeps lanes decoupled and lets the data layer
> adapt its layout underneath. See [conventions](../contributing/conventions.md).

## Step 2 — Define the agent

In the same file, add the agent. It implements [`Agent`] **and** `Default` — and
nothing else. No `start`, no builders, no accessors: an agent's only jobs are
lane selection, GORNA negotiation, and lane dispatch.

```rust
use std::any::Any;
use std::time::Duration;

use khora_sdk::khora_core::agent::{
    Agent, AgentImportance, ExecutionPhase, ExecutionTiming,
};
use khora_sdk::khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget,
    StrategyId, StrategyOption,
};
use khora_sdk::khora_core::EngineContext;
use khora_sdk::khora_core::lane::{Lane, LaneContext};

#[derive(Default)]
pub struct HeartbeatAgent {
    /// The lane chosen for this frame, set by `apply_budget`.
    lane: Option<Box<dyn Lane>>,
    /// The strategy GORNA last issued us.
    current_strategy: StrategyId,
}

impl Agent for HeartbeatAgent {
    fn id(&self) -> AgentId {
        // Reuse an existing slot — `AgentId` is a fixed enum, so a custom
        // subsystem borrows the kind closest to its work.
        AgentId::Ecs
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        // We offer a single, cheap strategy. A richer subsystem would offer
        // several (e.g. Balanced vs LowPower) with different cost estimates.
        NegotiationResponse {
            strategies: vec![StrategyOption {
                id: StrategyId::Balanced,
                estimated_time: Duration::from_micros(10),
                estimated_vram: 0,
            }],
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        // Pick the lane that matches the issued strategy. With one strategy,
        // there is one lane.
        self.current_strategy = budget.strategy_id;
        self.lane = Some(Box::new(HeartbeatLane::default()));
    }

    fn execute(&mut self, _context: &mut EngineContext<'_>) {
        if let Some(lane) = self.lane.as_ref() {
            let mut ctx = LaneContext::new();
            if let Err(e) = lane.execute(&mut ctx) {
                log::error!("heartbeat lane failed: {e}");
            }
        }
    }

    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id(),
            health_score: 1.0,
            current_strategy: self.current_strategy,
            is_stalled: false,
            message: "heartbeat ok".to_owned(),
        }
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            // TRANSFORM is the per-frame logic/simulation phase.
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            priority: 0.5,
            importance: AgentImportance::Optional,
            fixed_timestep: None,
            dependencies: Vec::new(),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

Two things to internalise:

- The agent owns **exactly one** `LaneKind` and only ever selects a lane,
  negotiates, and dispatches. Any other method on an agent struct is a rule
  violation. See [conventions](../contributing/conventions.md).
- The lane it dispatches is chosen in `apply_budget` from the strategy GORNA
  issued — that is how a subsystem scales itself under load. See
  [GORNA](../concepts/gorna.md).

## Step 3 — Register the agent

Custom agents are registered through the **`AgentProvider`** trait your app already
implements. The DCC calls `register_agents` once at boot. An agent is registered
as an `Arc<Mutex<dyn Agent>>` with a priority.

In `main.rs`, add `mod heartbeat;` at the top, then fill in the (currently empty)
`register_agents`:

```rust
mod heartbeat;

use heartbeat::HeartbeatAgent;
// (Arc and Mutex are already imported from std::sync in your main.rs.)

impl AgentProvider for MyGame {
    fn register_agents(&self, dcc: &DccService, _runtime: &mut Runtime) {
        dcc.register_agent(Arc::new(Mutex::new(HeartbeatAgent::default())), 0.5);
    }
}
```

No special bootstrap is needed — the same `run_winit::<WinitWindowProvider,
MyGame>(...)` call from the first lesson picks up the registration.

> To restrict an agent to certain engine modes, use
> `dcc.register_agent_for_mode(agent, priority, modes)` instead.

## Step 4 — Run it

```bash
cargo run -p my-first-game
```

**You should now see** the lit scene from the first lesson, and — once the engine
enters the simulation loop — a steady stream of `heartbeat: tick` lines in the
terminal, one per simulation frame. Your subsystem is now part of the frame.

If you see no ticks, confirm `mod heartbeat;` is declared in `main.rs` and that
`register_agents` is the one being compiled (Rust will warn about an unused agent
otherwise).

## Next steps

You have added a real subsystem to the engine's frame loop. To go further:

- [How-to recipes](../how-to/index.md) — add a lane to an *existing* agent, add an
  agent, add an ECS component, add a shader.
- [Agents and Lanes](../concepts/agents-and-lanes.md) — the full contract: why
  agents stay strategists and lanes stay executors.
- [GORNA](../concepts/gorna.md) — how budgets are negotiated, so your `negotiate`
  and `apply_budget` offer meaningful strategies.
