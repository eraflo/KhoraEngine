# Extending Khora

Custom agents, custom lanes, custom backends. A worked example.

- Document — Khora Extending v1.0
- Status — Tutorial
- Date — May 2026

---

## Contents

1. When to extend
2. The extension surface
3. Worked example — adding an AI agent
4. Adding a custom lane
5. Adding a custom backend
6. Decisions
7. Open questions

---

## 01 — When to extend

Khora ships with five agents, ~15 lanes, and four trait surfaces (`RenderSystem`, `PhysicsProvider`, `AudioDevice`, `LayoutSystem`). For most game work, that is enough.

You extend Khora when:

- **You need a new subsystem with multiple performance strategies.** AI, scripting, networking — anything where a simulation step has cost variants. → New agent + new lanes.
- **You need a new strategy in an existing subsystem.** A new render technique, a new audio mixer. → New lane in the existing agent.
- **You need to swap a backend.** Vulkan-direct rendering, custom physics solver, alternative audio API. → New `khora-infra/<area>/<backend>/` implementing the existing trait.
- **You need a fixed-behavior on-demand subsystem.** No GORNA, just "do this when called." → A service, not an agent.

If none of those describe your need, you probably do not need to extend the engine — you need a component, a system, or game-side code.

## 02 — The extension surface

The contracts you implement, in order of how often they are used.

| Trait | Crate | When |
|---|---|---|
| `Component` | `khora-core` (via `#[derive(Component)]`) | New ECS component |
| `Lane` | `khora-core` | New strategy for an existing or new agent |
| `Agent` | `khora-core` | New negotiating subsystem |
| `AssetDecoder<A>` | `khora-lanes` | New asset format |
| `RenderSystem` | `khora-core` | New graphics backend |
| `PhysicsProvider` | `khora-core` | New physics backend |
| `AudioDevice` | `khora-core` | New audio backend |
| `LayoutSystem` | `khora-core` | New UI layout backend |

All of these are pure Rust traits — no macros required, no FFI. Compile errors guide you.

## 03 — Worked example — adding an AI agent

Suppose your game needs an AI subsystem with three quality strategies (Full, Reduced, Disabled) negotiable through GORNA. This is the canonical "new agent" path.

### Step 1 — Define the lane(s)

Each strategy is a lane. Three strategies, three lanes.

```rust
// crates/my-game-ai/src/lanes/full_ai.rs
use khora_core::lane::{Lane, LaneContext, LaneError};

#[derive(Default)]
pub struct FullAiLane {
    behavior_tree: BehaviorTreeRunner,
}

impl Lane for FullAiLane {
    fn execute(&mut self, ctx: &mut LaneContext<'_>) -> Result<(), LaneError> {
        let world = ctx.get::<World>().ok_or(LaneError::MissingData)?;
        for (entity, ai) in world.query::<(EntityId, &mut AiState)>() {
            self.behavior_tree.tick(entity, ai, /* full deliberation */);
        }
        Ok(())
    }

    fn strategy_name(&self) -> &'static str { "FullAi" }

    fn estimate_cost(&self, ctx: &LaneContext<'_>) -> f32 {
        // Cost scales with the number of AI agents in the world
        let world = ctx.get::<World>();
        let count = world.map(|w| w.query::<&AiState>().count()).unwrap_or(0);
        (count as f32 / 100.0).min(1.0)
    }
}
```

Repeat for `ReducedAiLane` (lower-quality decisions, smaller cost) and `DisabledAiLane` (no work).

### Step 2 — Define the agent

The agent owns lane selection. It implements `Agent`, plus `Default`. Nothing else.

```rust
// crates/my-game-ai/src/agent.rs
use khora_core::agent::*;
use khora_core::control::gorna::*;

#[derive(Default)]
pub struct AiAgent {
    current_lane: Option<Box<dyn Lane>>,
}

impl Agent for AiAgent {
    fn id(&self) -> AgentId {
        AgentId::Custom("ai".to_string())
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_modes: vec![EngineMode::Playing],
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            priority: 0.7,
            importance: AgentImportance::Important,
            fixed_timestep: None,
            dependencies: vec![
                AgentDependency {
                    target: AgentId::Physics,
                    kind: DependencyKind::Soft,
                    condition: Some(DependencyCondition::IfTargetActive),
                },
            ],
        }
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![
                StrategyOption::new("FullAi", Duration::from_micros(2000), 0),
                StrategyOption::new("ReducedAi", Duration::from_micros(700), 0),
                StrategyOption::new("DisabledAi", Duration::ZERO, 0),
            ],
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.current_lane = Some(match budget.strategy_id.as_str() {
            "FullAi" => Box::new(FullAiLane::default()),
            "ReducedAi" => Box::new(ReducedAiLane::default()),
            _ => Box::new(DisabledAiLane::default()),
        });
    }

    fn execute(&mut self, ctx: &mut EngineContext<'_>) {
        if let Some(lane) = self.current_lane.as_mut() {
            let mut lane_ctx = LaneContext::from(ctx);
            let _ = lane.execute(&mut lane_ctx);
        }
    }

    fn report_status(&self) -> AgentStatus {
        AgentStatus::healthy()
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}
```

Note the absence of methods like `start`, `stop`, builders, or accessors. Agents implement only `Agent` plus `Default`. Construction goes through `Default::default()`. Free helper functions in the same file are fine.

### Step 3 — Register the agent

Custom agents are registered through the `AgentProvider` trait your app already implements. The DCC calls `register_agents` once during boot:

```rust
impl AgentProvider for MyGame {
    fn register_agents(&self, dcc: &DccService, services: &mut ServiceRegistry) {
        // Register an agent active in all engine modes
        dcc.register_agent(AiAgent::default(), /* priority */ 0.7);

        // Or restrict to specific modes
        // dcc.register_agent_for_mode(EditorOnlyAgent::default(), 0.5, &[EngineMode::Editor]);
    }
}
```

No special engine bootstrap is needed — the same `run_winit::<W, MyGame>(...)` call you write for any Khora app picks up the registration.

### Step 4 — Test it

Add a unit test that constructs the agent, calls `negotiate` with a synthetic request, applies a returned budget, and verifies the right lane was chosen. Add an integration test that runs a full frame with the agent registered and asserts no panics.

```rust
#[test]
fn agent_picks_full_when_budget_is_high() {
    let mut agent = AiAgent::default();
    let response = agent.negotiate(NegotiationRequest::high_budget());
    let chosen = response.strategies.iter().find(|s| s.id == "FullAi").unwrap();
    agent.apply_budget(ResourceBudget::for_strategy("FullAi"));
    assert_eq!(agent.current_lane.as_ref().unwrap().strategy_name(), "FullAi");
}
```

## 04 — Adding a custom lane

If you want a new strategy in an *existing* agent — for example, a new render technique — the path is shorter:

1. Implement `Lane` in `crates/khora-lanes/src/render_lane/your_strategy.rs`.
2. Add a WGSL shader file under `crates/khora-lanes/src/render_lane/shaders/`.
3. Wire it into `RenderAgent::negotiate` as a new `StrategyOption`.
4. Add a switch case in `RenderAgent::apply_budget` to instantiate it.
5. Write a test — either a unit test for the lane in isolation, or an integration test through the agent.

The hard part is the shader and the cost estimate. Everything else is mechanical.

## 05 — Adding a custom backend

To swap, say, the graphics backend:

1. Create `crates/khora-infra/src/graphics/<your-backend>/`.
2. Implement `RenderSystem` and the device contract from `khora-core`.
3. Register the new system at SDK init: `services.register::<Arc<dyn RenderSystem>>(Arc::new(YourSystem::new()))`.
4. Run the workspace tests — render lanes hold `Arc<dyn GraphicsDevice>`, so they pick up your backend transparently.
5. Run the sandbox to confirm visual parity.

The default wgpu backend is a reference implementation, not a commitment. Use it as a template. Vulkan-direct, Metal-direct, and software (for tests) backends are all valid targets.

The same pattern works for:

- `PhysicsProvider` — replace Rapier3D.
- `AudioDevice` — replace CPAL.
- `LayoutSystem` — replace Taffy.

## Decisions

### We said yes to
- **Extension through traits, not callbacks.** A trait implementation is a unit of code, testable, swappable, IDE-friendly. Callback registries are not.
- **The agent rule applies to custom agents too.** Custom agents implement only `Agent` + `Default`. No exceptions.
- **Custom strategies as new lanes.** The cleanest unit of new code is a new lane. Adding strategies through configuration would dilute the Lane abstraction.
- **Backends as trait implementations in `khora-infra`.** Custom backends live in the same place as the defaults. Nothing about backend implementation is special.

### We said no to
- **Plugin DLLs at v1.** Compile-time integration is the model. Runtime plugin loading is on the [Roadmap](./roadmap.md) but not the v1 model.
- **A "lite" Agent trait for simple cases.** Every agent participates in negotiation. There is no shortcut that skips GORNA — that is what services are for.
- **Custom phases at v1.** `ExecutionPhase::custom(id)` exists but the surrounding tooling (editor visibility, telemetry naming) is incomplete.

## Open questions

1. **Agent registration API.** `EngineConfig::register_agent` is illustrative, not stable. The pattern is settling alongside `khora-plugins`.
2. **Plugin DLL ABI.** Hot-loaded plugin agents need a stable ABI we have not yet committed to.
3. **Async lanes.** Asset streaming and AI deliberation both want `async` execution. The current sync-only contract is a known constraint.

---

*Next: the global decisions log. See [Decisions](./decisions.md).*
