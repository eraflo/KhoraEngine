# Add an agent

This guide shows you how to **add a strategist agent** — a subsystem that negotiates
a budget through GORNA, selects one lane, and dispatches it every frame.

**Prerequisites:** you have completed
[Extending the engine](../tutorials/extending-the-engine.md) and read
[Agents](../concepts/agents-and-lanes.md).

> **Use an agent only when the subsystem needs GORNA negotiation.** Work that just
> runs deterministically each tick belongs in a direct service (`AssetService`,
> `EcsMaintenance`, a `DataSystem`), not an agent.

## Step 1 — Implement `Agent` and `Default` — and nothing else

An agent struct implements **exactly two traits**: [`Agent`](../concepts/agents-and-lanes.md) and
`Default`. No `start`/`stop`, no builders, no accessors — those would violate the
agent contract. Construction is always `Default::default()`. The agent owns **one**
`LaneKind` and stores only its own GORNA/strategy state; every shared service is
looked up from the `Runtime` each frame.

```rust
use std::any::Any;
use std::time::Duration;

use khora_core::agent::{Agent, AgentImportance, ExecutionPhase, ExecutionTiming};
use khora_core::context::EngineContext;
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget,
    StrategyId, StrategyOption,
};
use khora_core::lane::{Lane, LaneContext};

#[derive(Default)]
pub struct FoliageAgent {
    lane: Option<Box<dyn Lane>>,
    current_strategy: StrategyId,
}
```

## Step 2 — Pick an `AgentId`

`AgentId` is a **fixed enum** — there is no `Custom` variant. A new subsystem reuses
the slot closest to its work. Foliage rendering, for example, reuses
`AgentId::Renderer`. The variants are: `Renderer`, `ShadowRenderer`, `Overlay`,
`Physics`, `Ecs`, `Ui`, `Audio`, `Asset`.

```rust
impl Agent for FoliageAgent {
    fn id(&self) -> AgentId {
        AgentId::Renderer
    }
```

## Step 3 — Offer strategies in `negotiate`, pick a lane in `apply_budget`

`negotiate` returns the strategies this agent can run with their estimated cost; the
DCC issues one back. `apply_budget` records the chosen strategy and selects the
matching lane — this is how the subsystem scales itself under load.

```rust
    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![
                StrategyOption {
                    id: StrategyId::HighPerformance,
                    estimated_time: Duration::from_micros(800),
                    estimated_vram: 4 * 1024 * 1024,
                },
                StrategyOption {
                    id: StrategyId::LowPower,
                    estimated_time: Duration::from_micros(200),
                    estimated_vram: 1024 * 1024,
                },
            ],
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.current_strategy = budget.strategy_id;
        // Pick the lane matching the issued strategy.
        // self.lane = Some(Box::new(...));
    }
```

## Step 4 — Dispatch the lane in `execute`

Read inputs from `context.bus`, build a `LaneContext`, dispatch the lane, and write
outputs to `context.deck`. The agent does **only** lane selection and dispatch — all
real work lives in the lane (see [Add a lane](./add-a-lane.md)).

```rust
    fn execute(&mut self, context: &mut EngineContext<'_>) {
        let Some(lane) = self.lane.as_ref() else { return };
        let mut ctx = LaneContext::new();
        // … insert Views read from context.bus into ctx …
        if let Err(e) = lane.execute(&mut ctx) {
            log::error!("foliage lane failed: {e}");
        }
        let _ = context;
    }

    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id(),
            health_score: 1.0,
            current_strategy: self.current_strategy,
            is_stalled: false,
            message: "foliage ok".to_owned(),
        }
    }
```

## Step 5 — Declare execution timing

`execution_timing` tells the scheduler **when** the agent runs. The real phases are
`INIT`, `OBSERVE`, `TRANSFORM`, `MUTATE`, `OUTPUT`, `FINALIZE`. `OUTPUT` is the
render/present phase; `TRANSFORM` is per-frame logic. Mark work the scheduler may
drop under budget pressure as `AgentImportance::Optional` — only `Optional` is
negotiable; `Critical` and `Important` always run.

```rust
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::OUTPUT],
            default_phase: ExecutionPhase::OUTPUT,
            priority: 0.7,
            importance: AgentImportance::Optional,
            fixed_timestep: None,
            dependencies: Vec::new(),
        }
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}
```

## Step 6 — Register the agent via `AgentProvider`

Game and host code register agents through the `AgentProvider::register_agents` hook.
Wrap the agent in `Arc<Mutex<…>>` and hand it to the DCC with a priority. Use
`register_agent_for_mode` to restrict it to specific engine modes.

```rust
impl AgentProvider for MyGame {
    fn register_agents(&self, dcc: &DccService, _runtime: &mut Runtime) {
        dcc.register_agent(Arc::new(Mutex::new(FoliageAgent::default())), 0.7);
    }
}
```

## Step 7 — Verify it works

```bash
cargo test --workspace
cargo run -p sandbox
```

Confirm the agent's `report_status` message and its lane's effect appear in the running
frame. For render agents, check there are no wgpu/Vulkan validation errors.

## Related

- [Agents](../concepts/agents-and-lanes.md) — the full agent contract and lifecycle.
- [GORNA](../concepts/gorna.md) — how budgets are negotiated and issued.
- [Control GORNA adaptation](./control-gorna-adaptation.md) — pin or bound this
  agent's strategy from host code.
