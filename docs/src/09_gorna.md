# GORNA — Goal-Oriented Resource Negotiation & Allocation

GORNA is the protocol through which agents and the DCC negotiate resource budgets. It ensures every agent gets the resources it needs while respecting global constraints.

## Protocol Phases

```mermaid
flowchart LR
    A[Awareness] -->|Collect metrics| B[Analysis]
    B -->|Run heuristics| C[Negotiation]
    C -->|Collect strategies| D[Arbitration]
    D -->|Apply budgets| E[Application]
    E -->|Next tick ~50ms| A
```

| Phase | Duration | Action |
|-------|----------|--------|
| **Awareness** | Instant | Collect telemetry from agents and hardware monitors |
| **Analysis** | ~1ms | Run heuristic engine — thermal, battery, load analysis |
| **Negotiation** | ~2ms | Request strategy options from each agent |
| **Arbitration** | ~1ms | Select optimal strategy per agent within budget |
| **Application** | Instant | Call `apply_budget()` on each agent, send to BudgetChannel |

## Data Structures

### NegotiationRequest

```rust
pub struct NegotiationRequest {
    pub target_latency: Duration,       // e.g., 16.6ms for 60 FPS
    pub priority_weight: f32,           // 0.0 to 1.0
    pub constraints: ResourceConstraints,
    pub current_phase: EnginePhase,     // Boot, Menu, Simulation, Background
    pub agent_timing: ExecutionTiming,  // Agent's declared timing
}
```

### NegotiationResponse

```rust
pub struct NegotiationResponse {
    pub strategies: Vec<StrategyOption>,
    pub timing_adjustment: Option<TimingAdjustment>,
}

pub struct StrategyOption {
    pub id: StrategyId,         // LowPower, Balanced, HighPerformance
    pub estimated_time: Duration,
    pub estimated_vram: u64,
}
```

### ResourceBudget

```rust
pub struct ResourceBudget {
    pub strategy_id: StrategyId,
    pub time_limit: Duration,
    pub memory_limit: Option<u64>,
    pub vram_limit: Option<u64>,
    pub extra_params: HashMap<String, f64>,
}
```

## Heuristics

The DCC's heuristic engine evaluates:

| Heuristic | Input | Output |
|-----------|-------|--------|
| Thermal throttling | GPU/CPU temperature | Reduce budget multiplier |
| Battery level | Battery percentage | Reduce budget on low battery |
| Death spiral detection | Consecutive over-budget frames | Force LowPower strategy |
| Load balancing | CPU/GPU utilization | Rebalance time budgets |

> [!WARNING]
> **GORNA cannot force phases.** It can only suggest importance changes (`TimingAdjustment`). Agents always control which phases they run in via `allowed_phases`.

## Compliance Table

| Agent | Negotiates | Applies Budget | Reports Status |
|-------|-----------|----------------|----------------|
| RenderAgent | ✅ 3 strategies | ✅ Switches lane strategy | ✅ Frame time, draws, lights |
| PhysicsAgent | ✅ 3 strategies | ✅ Adjusts fixed timestep | ✅ Step time, bodies, colliders |
| UiAgent | ✅ 1 strategy | ✅ (no-op, single strategy) | ✅ Node count, text count |
| AudioAgent | ✅ 3 strategies | ✅ Adjusts max sources | ✅ Source count, frame |
