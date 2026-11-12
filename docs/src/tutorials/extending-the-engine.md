# Extending the engine

So far you have written *game* logic. This lesson crosses into *engine* work: you
will add your own subsystem — a **family** with one **lane** that does its work —
register it, and watch the scheduler run it. By the end the engine's frame loop
will run your code and you will see it log as the frames go by.

The example is deliberately tiny: a "heartbeat" subsystem that counts frames and
logs a line at a steady cadence. The mechanics are exactly the ones a real
subsystem (AI, audio, networking) uses.

Start from a working Khora app — the project from
[Your first game](./your-first-game.md) is fine.

## What you are building

Three pieces, all in one file:

- A **lane** — the executor. It implements the `Lane` trait and contains the
  actual work. A lane offers one or more **levels** — the same work at several
  costs — and records at the one it is given.
- A **family** — what its lanes share: the outputs they write and the name of the
  agent that negotiates for them. Several lanes of one family are alternatives
  that can stand in for each other.
- A **registration** — one `register_agent!`: the family's phase in the frame,
  its importance and its lanes.

You do not write an agent. The engine builds the family's agent, the one
`LaneAgent`, from the registration: it negotiates a budget through GORNA, chooses
the level that fits, and installs it as a layer that the scheduler runs every
frame.

> Why a family and a lane? The agent *decides*, the lane *does*. This split is
> what lets the engine swap quality strategies under load. See
> [Agents and Lanes](../concepts/agents-and-lanes.md).

## Step 1 — Add the dependencies

The lane derives and the registration live in engine crates the SDK does not
re-export, and the derives name `khora_core` directly. Add them to `Cargo.toml`,
next to `khora-sdk`:

```toml
[dependencies]
khora-sdk = { path = "crates/khora-sdk" }
khora-core = { path = "crates/khora-core" }
khora-macros = { path = "crates/khora-macros" }
khora-agents = { path = "crates/khora-agents" }
log = "0.4"
anyhow = "1"
env_logger = "0.11"
```

## Step 2 — Define the family

Create `src/heartbeat.rs`. Start with what the subsystem reads and writes, and
the family that ties them together:

```rust
use std::time::Duration;

use khora_agents::registered_agent::RegisteredAgent;
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{Lane, LaneError, LaneReport};
use khora_macros::{LaneInputs, LaneOutputs};

/// How often the heartbeat speaks: one line every `every` frames.
pub struct HeartbeatSettings {
    pub every: u64,
}

/// The beats counted so far, handed on in the deck.
#[derive(Debug, Clone, Default)]
pub struct Beats(pub u64);

/// The heartbeat family: its name, the agent that negotiates for it, and
/// the outputs every one of its lanes writes.
pub struct HeartbeatFamily;

impl LaneFamily for HeartbeatFamily {
    const NAME: &'static str = "heartbeat";
    const AGENT: AgentId = AgentId::new("heartbeat");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = HeartbeatOutputs;
}

/// What every heartbeat lane writes.
#[derive(LaneOutputs)]
pub struct HeartbeatOutputs {
    /// The beats counted so far.
    #[deck]
    pub beats: Beats,
}
```

The family's `AGENT` is the name its agent goes by; boot refuses a name another
family already uses. `Semantics::Presentation` says a cheaper level may give a
lesser result — a log line is presentation, never simulation. The `#[deck]`
output is a typed slot of the `OutputDeck` that later phases can read.

## Step 3 — Define the lane

In the same file, add the lane's inputs, its one level, and the lane itself:

```rust
/// What the heartbeat lane reads each frame.
#[derive(LaneInputs)]
pub struct HeartbeatInputs<'f> {
    /// The settings the game installed in the runtime.
    #[resource]
    pub settings: &'f HeartbeatSettings,
}

/// The lane's one level.
const LEVELS: [Level; 1] = [Level {
    id: StrategyId::Balanced,
    name: "heartbeat",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

/// The heartbeat lane: counts the frames and logs every `every`-th.
#[derive(Default)]
pub struct HeartbeatLane {
    frames: u64,
}

impl Lane for HeartbeatLane {
    type Family = HeartbeatFamily;
    type Inputs<'f> = HeartbeatInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &HeartbeatInputs<'_>) -> Cost {
        Cost {
            time: PerDevice {
                cpu: Duration::from_micros(1),
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut HeartbeatInputs<'_>,
        out: &mut HeartbeatOutputs,
    ) -> Result<LaneReport, LaneError> {
        self.frames += 1;
        if self.frames % inputs.settings.every.max(1) == 0 {
            log::info!("heartbeat: tick {}", self.frames);
        }
        out.beats = Beats(self.frames);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}
```

Each field of the inputs names where it comes from. `#[resource]` reads a runtime
entry without locking it; a real lane also reads **Views** a `Flow` published on
the `LaneBus` (`#[bus]`), never the world directly. The lane only sees what its
inputs name, and writes only its family's outputs.

`cost` prices a level on the frame's inputs; GORNA compares these prices with the
budget. `record` does the frame's work at the level it is given.

> Why read Views, not the world? It keeps lanes decoupled and lets the data layer
> adapt its layout underneath. See [conventions](../contributing/conventions.md).

## Step 4 — Register the family

At the end of `heartbeat.rs`, build the registration and submit it. The engine
collects every submitted registration at boot, the game's with its own:

```rust
/// The heartbeat family's registration.
pub const HEARTBEAT: AgentRegistration = khora_core::register_agent! {
    family: HeartbeatFamily,
    // TRANSFORM is the per-frame logic phase.
    phase: ExecutionPhase::TRANSFORM,
    // Optional: the first work skipped under budget pressure.
    importance: AgentImportance::Optional,
    after: [],
    fixed_step: None,
    lanes: [HeartbeatLane],
};

khora_sdk::inventory::submit! {
    RegisteredAgent(HEARTBEAT)
}
```

Every lane listed must name `HeartbeatFamily` as its `Family`, or this does not
compile. `fixed_step: Some(step)` would run the family at a fixed step instead of
every frame.

## Step 5 — Provide the settings

The lane reads `HeartbeatSettings` from the runtime, and boot refuses the family
if the runtime does not hold it. Your app installs it in `EngineApp::provide`,
which runs before the families are built. In `main.rs`, add `mod heartbeat;` at
the top, then `provide` to your `impl EngineApp`:

```rust
mod heartbeat;

impl EngineApp for MyGame {
    // … window_config, new, setup and update as before …

    fn provide(&self, runtime: &mut Runtime) {
        runtime
            .resources
            .insert(heartbeat::HeartbeatSettings { every: 60 });
    }
}
```

No special bootstrap is needed — the same `run_winit::<WinitWindowProvider,
MyGame>(...)` call from the first lesson boots the family with the engine's.

> The family starts on. To switch it off and on while the game runs, call
> `deactivate(id)` and `activate(id)` on the `Arc<Activation>` resource
> (`khora_sdk::Activation`) with `AgentId::new("heartbeat")`; the switch lands at
> the next frame boundary.

## Step 6 — Test it

You can run the family for one frame without a window or a scheduler: build its
agent with `lane_agent` and drive it with `run_frame`. Add to `heartbeat.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use khora_agents::lane_agent::{lane_agent, run_frame};
    use khora_core::agent::Agent;
    use khora_core::gpu::DeviceCaps;
    use khora_core::lane::io::FrameId;
    use khora_core::lane::{LaneBus, OutputDeck};
    use khora_core::{EngineContext, Runtime};

    use super::{Beats, HeartbeatSettings, HEARTBEAT};

    #[test]
    fn one_frame_counts_one_beat() {
        let mut runtime = Runtime::new();
        runtime.resources.insert(HeartbeatSettings { every: 60 });
        let runtime = Arc::new(runtime);
        let mut agent = lane_agent(&HEARTBEAT, &DeviceCaps::default());

        let mut bus = LaneBus::new();
        bus.set_frame(FrameId(1));
        let mut deck = OutputDeck::new();
        let permit = agent.contention();
        let id = agent.id();
        {
            let mut context =
                EngineContext::for_agent(Arc::clone(&runtime), &bus, &mut deck, &permit, Some(id));
            run_frame(&mut agent, &mut context);
        }

        assert_eq!(deck.get::<Beats>().map(|beats| beats.0), Some(1));
    }
}
```

The first `run_frame` installs the family's level and runs it, so the lane's
output is in the deck after one call.

```bash
cargo test -p my-first-game
```

## Step 7 — Run it

```bash
RUST_LOG=info cargo run -p my-first-game
```

**You should now see** the lit scene from the first lesson, and — once the engine
enters the frame loop — a `heartbeat: tick 60`, `heartbeat: tick 120`, … line in
the terminal every 60 frames. Your subsystem is now part of the frame.

If you see no ticks, confirm `mod heartbeat;` is declared in `main.rs` and that
`RUST_LOG` lets `info` through. If the engine refuses to boot, read the error: it
names the family and what is missing, such as a resource `provide` did not
install.

## Next steps

You have added a real subsystem to the engine's frame loop. To go further:

- [How-to recipes](../how-to/index.md) — add a lane to an *existing* family, add a
  family, add an ECS component, add a shader.
- [Agents and Lanes](../concepts/agents-and-lanes.md) — the full contract: why
  agents stay strategists and lanes stay executors.
- [GORNA](../concepts/gorna.md) — how budgets are negotiated, so your levels and
  their `cost` give the agent a meaningful choice.
