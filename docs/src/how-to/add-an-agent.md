# Add a subsystem (a family)

This guide shows you how to **add a subsystem** that negotiates a budget through
GORNA: a **family** — its lanes, its outputs and one `register_agent!`
registration. You do not write the agent. Every family's agent is the engine's
`LaneAgent`, built from the registration: it offers the family's levels to GORNA,
chooses the one that fits its envelope, and installs it as a
[layer](../concepts/agents-and-lanes.md#lanes-are-layers) that the scheduler runs
every frame.

**Prerequisites:** you have completed
[Extending the engine](../tutorials/extending-the-engine.md) and read
[Agents and Lanes](../concepts/agents-and-lanes.md).

> **Use a family only when the subsystem has something to negotiate.** Work that
> just runs deterministically each tick belongs in a direct service
> (`AssetService`, `EcsMaintenance`) or a `DataSystem`, not in a family.

> **Never write an agent type.** A family is its lanes and one registration; its
> agent is always `LaneAgent` (`crates/khora-agents/src/agent/mod.rs`), built by
> `lane_agent`. An agent implements only `Agent` and has no `execute`: the
> scheduler runs the family's place, and wakes the agent only for an event.

## Step 1 — Define the family

A family names what all its lanes share: the outputs they write, and the agent
that negotiates for them. It implements `LaneFamily`
(`crates/khora-core/src/lane/family.rs`):

- `NAME` — the family's name, as boot errors and telemetry print it;
- `AGENT` — the `AgentId` the family negotiates under. It must be unique: boot
  refuses two families under one agent, and the engine's families already use
  `renderer`, `shadows`, `skybox`, `grid`, `wireframe`, `gizmo`, `ui`, `audio`,
  `physics`, `script` and `loading`;
- `SEMANTICS` — what a cheaper level may change. `Presentation`: a cheaper level
  may look or sound worse. `Exact`: every level gives the same result, and boot
  refuses a level whose `impact` is not `Impact::None`;
- `Outputs` — the outputs every lane of the family writes, a struct deriving
  `LaneOutputs`.

The audio family, from `crates/khora-lanes/src/audio_lane/audio_family.rs`:

```rust
use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_data::flow::AudioPlaybackWriteback;
use khora_macros::LaneOutputs;

/// The audio family. Its output is presentation: a cheaper level mixes fewer
/// voices, never a different playback.
pub struct AudioFamily;

impl LaneFamily for AudioFamily {
    const NAME: &'static str = "audio";
    const AGENT: AgentId = khora_core::audio::AUDIO; // AgentId::new("audio")
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = AudioOutputs;
}

/// What every audio lane writes: one playback update per source.
#[derive(LaneOutputs)]
pub struct AudioOutputs {
    #[deck]
    pub playback: AudioPlaybackWriteback,
}
```

Each output field names where it goes: `#[deck]` (an owned value a later phase
reads from the `OutputDeck`), `#[pass]` (GPU commands for a pass slot, as the
skybox family's `Pass<'f, SkyboxPassSlot>` in
`crates/khora-lanes/src/skybox_lane/skybox_family.rs`) or `#[staged]` (what a
background family hands on).

## Step 2 — Write its lanes

Each lane implements `khora_core::lane::Lane` with `type Family = YourFamily`:
the levels it offers, what a level costs, and how it records a frame's work. A
family has at least one lane; several lanes are alternatives the agent chooses
between (the render family has `SimpleUnlitLane`, `LitForwardLane` and
`ForwardPlusLane`). See [Add a lane](./add-a-lane.md).

## Step 3 — Register the family's agent

One `register_agent!` (`crates/khora-core/src/agent/registration.rs`) builds the
family's registration. The engine's live in `crates/khora-agents/src/` — one
`<domain>_agent/agent.rs` per domain (the grid, wireframe and gizmo families
share `overlay_agent`) — and are collected at link time. The skybox's,
from `crates/khora-agents/src/skybox_agent/agent.rs`:

```rust
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_lanes::skybox_lane::{SkyboxFamily, SkyboxLane};

use crate::registered_agent::RegisteredAgent;

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: SkyboxFamily,
        // The sky loads the colour and depth the opaque scene pass drew.
        phase: ExecutionPhase::OUTPUT,
        // Not debug viz: the background survives budget pressure.
        importance: AgentImportance::Important,
        after: [crate::render_agent::RENDERER],
        fixed_step: None,
        lanes: [SkyboxLane],
    })
}
```

The fields, in the order the macro takes them:

| Field | What it says |
|---|---|
| `family` | The `LaneFamily` type. Every lane listed must name it as its `Lane::Family`, or the registration does not compile. |
| `phase` | The phase the family's place runs in: `INIT`, `OBSERVE`, `TRANSFORM` (per-frame logic and simulation), `MUTATE`, `OUTPUT` (rendering), `FINALIZE`, a game's own phase, or `BACKGROUND` for work that spans frames. |
| `importance` | `Critical`, `Important` or `Optional`. Only `Optional` is negotiable: it is the work the scheduler skips under budget pressure. |
| `after` | The agents this family runs after within its phase, by `AgentId`. |
| `fixed_step` | `None` to run every frame, or `Some(step)` to run at a fixed step. The step is held by the family's place (`Place::fixed_step`); for physics the engine replaces the default with the game's `physics.step` at boot. |
| `default_level` | Optional, after `fixed_step`: the `StrategyId` the family starts on. Without it the family starts on its menu's first row. |
| `lanes` | The family's lanes. Each is built with `Default::default()`. |

The render family names a default level, from
`crates/khora-agents/src/render_agent/agent.rs`:

```rust
khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: RenderFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Critical,
        after: [],
        fixed_step: None,
        // The reference image until the DCC lends an envelope.
        default_level: StrategyId::Balanced,
        lanes: [SimpleUnlitLane, LitForwardLane, ForwardPlusLane],
    })
}
```

In the engine, add the family's `<family>_agent` module to
`crates/khora-agents/src/lib.rs`. A game registers its family the same way, from
its own crate; [Extending the engine](../tutorials/extending-the-engine.md) shows
the dependencies it needs.

Boot checks every registration together, before the first frame, and refuses the
set with a `BootError` naming the family when, among others: two registrations
name one family or one agent; a family offers no level, or one `StrategyId` twice;
no lane runs on the device; a lane reads a resource the runtime does not hold; a
lane reads a deck slot no earlier phase writes.

## Step 4 — Provide what the lanes read

A lane reads runtime resources through its `#[resource]` and `#[locked]` inputs.
Boot refuses the family if the runtime does not hold them
(`BootError::UnknownResource`). The engine installs its own in the bootstrap (the
skybox lane reads the `IblBaker` that way); an app installs its own in
`EngineApp::provide`, which runs before the families are built. For a lane with a
`#[resource] settings: &'f FoliageSettings` input:

```rust
impl EngineApp for MyGame {
    // …
    fn provide(&self, runtime: &mut Runtime) {
        runtime.resources.insert(FoliageSettings::default());
    }
}
```

## Step 5 — What the agent does for you

You write none of this; it is what `LaneAgent` does with your registration:

- **`offer`** — every level of every lane the device supports is a row of its
  menu, priced by the lanes' `cost`. Each epoch the DCC asks for the offer.
- **`apply`** — the DCC lends an envelope (time per device, memory), with any row
  a policy pins or allows; the agent chooses the row that fits.
- **`report`** — after each run the DCC reads a `Report`: the row that ran, its
  measured time, the health, whether it `stalled`, and the lane's `message`.
- **`place`** and **`wake`** — the chosen row runs as a layer at the family's
  place. The scheduler runs it every frame without the agent, and wakes the agent
  only for an event: a new envelope, a price that no longer fits, a failed lane, a
  refused input. The agent then prepares the new layer beside the installed one,
  swapped in at the next frame start.
- **`contention`** — what the installed layer's lanes declare: deck slots written
  and read, resources read or locked. Families share a concurrent wave only
  when their contentions are disjoint, they run in the same phase, and neither
  comes `after` the other (a fixed-step family never shares one with a
  once-per-frame family); reads do not conflict with reads. A wave of one runs
  inline, a larger one on the worker pool.

## Step 6 — Switch it off and on

A family starts on. To switch it off and on at run time, call `deactivate(id)` and
`activate(id)` on the `Arc<Activation>` resource with the family's `AgentId`; the
switch lands at the next frame boundary. An app does this in
`EngineApp::before_frame` to have it hold for the whole frame.

## Step 7 — Verify it works

Drive the family in a test without the scheduler: build its agent with
`lane_agent` and run one frame with `run_frame`. From
`crates/khora-agents/tests/it/audio_family.rs`, simplified:

```rust
use std::sync::Arc;

use khora_agents::agent::{lane_agent, run_frame};
use khora_core::agent::Agent;
use khora_core::gpu::DeviceCaps;
use khora_core::lane::io::FrameId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::EngineContext;
use khora_data::flow::AudioPlaybackWriteback;

// `counting_mix`, `runtime_with`, `audio_registration` and `view_of_sources`
// are helpers of that test file: a mix bus that counts its blocks, a runtime
// holding it, the family's registration, and an `AudioView` of three sources.
let mix = counting_mix();
let runtime = Arc::new(runtime_with(&mix));
let mut agent = lane_agent(audio_registration(), &DeviceCaps::default());
let mut bus = LaneBus::new();
bus.set_frame(FrameId(1));
bus.publish(view_of_sources(3)); // the View the lane reads
let mut deck = OutputDeck::new();
let permit = agent.contention();
let id = agent.id();
{
    let mut context =
        EngineContext::for_agent(Arc::clone(&runtime), &bus, &mut deck, &permit, Some(id));
    run_frame(&mut agent, &mut context);
}
let playback = deck.get::<AudioPlaybackWriteback>().expect("the lane wrote its slot");
```

`run_frame` answers the first-frame wake, installs the default row and runs it,
so the family's outputs are in the deck after one call. Then:

```bash
cargo gate
cargo run -p sandbox
```

For a render family, check there are no wgpu/Vulkan validation errors.

## Related

- [Agents and Lanes](../concepts/agents-and-lanes.md) — families, places and layers.
- [GORNA](../concepts/gorna.md) — how envelopes are lent and rows chosen.
- [Add a lane](./add-a-lane.md) — the lanes a family chooses between.
- [Control GORNA adaptation](./control-gorna-adaptation.md) — pin or bound a
  family's row from host code.
