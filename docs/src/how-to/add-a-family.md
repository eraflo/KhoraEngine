# Add a subsystem (a family)

A subsystem that has several ways of doing its work, at different costs, is a
**family**: its lanes are those ways, and the engine negotiates its budget through
GORNA. This guide shows you how to add one to the engine. You write **a family,
its lanes and one registry line**; you never write the agent. Every family's agent
is the engine's `LaneAgent`, built from the registry line: it offers the family's
rows, chooses the one that fits its envelope, and installs it as a
[layer](../concepts/agents-and-lanes.md#lanes-are-layers) the scheduler runs every
frame.

**Prerequisites:** you have completed
[Extending the engine](../tutorials/extending-the-engine.md) (which adds a family
from a game's own crate) and read [Agents and Lanes](../concepts/agents-and-lanes.md)
and [Families and levels](../concepts/families-and-levels.md).

> **Use a family only when the subsystem has something to negotiate.** Work with no
> alternatives — it just runs, deterministically — is a service (`AssetService`,
> `EcsMaintenance`) or a `DataSystem`, not a family.

> **Never write an agent type.** `LaneAgent` (`crates/khora-agents/src/lane_agent/`)
> is the only implementation of `Agent` outside tests; the xtask check
> `only_lane_agent_implements_agent` refuses another. An agent has one family, and
> no `execute`: the scheduler runs the family's place.

## Step 1 — Make the folder

A family of the engine lives in `crates/khora-lanes/src/<family>/`:

| File | Holds |
|---|---|
| `mod.rs` | the module, re-exporting the family and its lanes |
| `family.rs` | the `LaneFamily` and its outputs |
| one file per lane | the lanes (a folder for a lane of several files) |
| `registration.rs` | the family's one `register_agent!` |

Add `pub mod <family>;` to `crates/khora-lanes/src/lib.rs`, and the pair
`("<family>", "<family>")` to `FAMILY_FOLDERS` in `xtask/tests/layout.rs`: the
layout checks read it to find every family's folder and tests. Model:
`crates/khora-lanes/src/audio/`.

## Step 2 — Define the family

A family names what all its lanes share: the outputs they write, and the agent
that negotiates for them. It implements `LaneFamily`
(`crates/khora-core/src/lane/contract/family.rs`):

- `NAME` — the family's name, as boot errors and telemetry print it;
- `AGENT` — the `AgentId` the family negotiates under. It must be unique: boot
  refuses two families under one agent. The engine's families use `renderer`,
  `shadows`, `skybox`, `grid`, `wireframe`, `gizmo`, `ui`, `audio`, `physics`,
  `script` and `loading`. Use `AgentId::new("<name>")`, or a constant in
  `khora-core` (`khora_core::audio::AUDIO`) when other crates name the family;
- `SEMANTICS` — what a cheaper level may change. `Presentation`: it may look or
  sound worse. `Exact`: every level gives the same result, and boot refuses a level
  whose `impact` is not `Impact::None`;
- `Outputs` — what every lane of the family writes, a struct deriving `LaneOutputs`.

The audio family, from `crates/khora-lanes/src/audio/family.rs`:

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
    const AGENT: AgentId = khora_core::audio::AUDIO;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = AudioOutputs;
}

/// What every audio lane writes: one playback update per source, drained by
/// the `audio_playback_writeback` `DataSystem`.
#[derive(LaneOutputs)]
pub struct AudioOutputs {
    /// The playback state of every source after this frame's mix.
    #[deck]
    pub playback: AudioPlaybackWriteback,
}
```

Each output field says where it goes:

| Attribute | Field type | Goes to |
|---|---|---|
| `#[pass]` | `Pass<'f, S>` | GPU commands for the pass slot `S`, submitted in the order `register_pass!` gives (as the skybox family's `Pass<'f, SkyboxPassSlot>`) |
| `#[deck]` | an owned value | a deck slot a later phase or a `Maintenance` `DataSystem` reads |
| `#[staged]` | `Vec<T>` or `Leases<T>` of a `Staged` type | the reserve, for a `BACKGROUND` family: applied at the boundary of the first frame after its run ends |

Boot refuses a `#[staged]` output in a frame phase, and a frame output in a
`BACKGROUND` family.

## Step 3 — Write its lanes

Each lane implements `khora_core::lane::Lane` with `type Family = YourFamily`: the
levels it offers, what a level costs, how it records a frame's work. A family has
at least one lane; several lanes are alternatives the agent chooses between (the
render family has `SimpleUnlitLane`, `LitForwardLane` and `ForwardPlusLane`). See
[Add a lane](./add-a-lane.md).

## Step 4 — Write the registry line

One `register_agent!` (`crates/khora-core/src/agent/registration.rs`), in the
folder's `registration.rs`, submitted to the engine's collection. The skybox's,
from `crates/khora-lanes/src/skybox/registration.rs`:

```rust
use super::{SkyboxFamily, SkyboxLane};
use khora_core::agent::{AgentImportance, ExecutionPhase};

use khora_core::agent::RegisteredAgent;

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: SkyboxFamily,
        // The sky loads the colour and depth the opaque scene pass drew.
        phase: ExecutionPhase::OUTPUT,
        // Not debug viz: the background survives budget pressure.
        importance: AgentImportance::Important,
        after: [crate::render::RENDERER],
        fixed_step: None,
        lanes: [SkyboxLane],
    })
}
```

The fields, in the order the macro takes them:

| Field | What it says |
|---|---|
| `family` | The `LaneFamily` type. Every lane listed must name it as its `Lane::Family`, or the line does not compile. |
| `phase` | The phase the family's place runs in: `INIT`, `OBSERVE`, `TRANSFORM` (per-frame logic and simulation), `MUTATE`, `OUTPUT` (rendering), `FINALIZE`, a game's own phase, or `BACKGROUND` for work that spans frames. |
| `importance` | `Critical`, `Important` or `Optional`. It weighs the family in GORNA; only `Optional` work is skipped when the frame runs late. Priority comes from the importance, never from the name. |
| `after` | The families this one runs after within its phase, by `AgentId`. |
| `fixed_step` | `None` to run every frame, or `Some(step)` to run at a fixed step. The step is held by the family's place; for physics the engine replaces the default with the game's `physics.step` at boot. |
| `default_level` | Optional, after `fixed_step`: the `StrategyId` the family starts on. Without it the family starts on its menu's first row. |
| `lanes` | The family's lanes. Each is built with `Default`. |

The render family names a default level, from
`crates/khora-lanes/src/render/registration.rs`:

```rust
khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: RenderFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Critical,
        after: [],
        fixed_step: None,
        default_level: StrategyId::Balanced,
        lanes: [SimpleUnlitLane, LitForwardLane, ForwardPlusLane],
    })
}
```

Boot collects every registration (`registered_agents()`), checks them together
before the first frame (`check_registrations`), and refuses the set with a
`BootError` naming the family when, among others: two registrations name one
family or one agent; a family offers no level, or one `StrategyId` twice; no lane
runs on the device; a lane reads a resource the runtime does not hold, or a deck
slot no earlier phase writes; the default level is on no lane; a `BACKGROUND` lane
reads or writes anything of a single frame.

## Step 5 — Provide what the lanes read

A lane reads runtime entries through its `#[resource]` and `#[locked]` inputs. Boot
refuses the family if the runtime does not hold them (`BootError::UnknownResource`).
The engine installs its own in the bootstrap (the skybox lane reads the `IblBaker`
that way); an app installs its own in `EngineApp::provide`, which runs before the
families are built. For a lane with a `#[resource] settings: &'f FoliageSettings`
input:

```rust
impl EngineApp for MyGame {
    // …
    fn provide(&self, runtime: &mut Runtime) {
        runtime.resources.insert(FoliageSettings::default());
    }
}
```

`provide` installs runtime entries only; an app never registers an agent.

## Step 6 — Put the data side in its bricks

What the lanes read and what their outputs become are not the family's job. Each
has its own brick:

| You need | Write | Registered with |
|---|---|---|
| a View the lanes read | a `Flow`, projecting the World | `register_flow!` |
| the World written back from a deck slot | a `DataSystem` in `TickPhase::Maintenance` | a `DataSystemRegistration` |
| an asset's copy on a device | a residency | `register_residency!` |
| GPU work in the frame's pass order | a pass slot | `register_pass!` |

A `DataSystem` or `Flow` doing the family's work declares `Role::Serves(<AgentId>)`,
so it is skipped while the family is off; one keeping a data invariant declares
`Role::Upkeep`. See [Add a flow](./add-a-flow.md).

## What you do not write

The registry line is all the wiring. Everything else is what `LaneAgent` does with
it:

- **The offer.** Every level of every lane the device supports is a row of its
  menu, priced by the lanes' `cost`.
- **The choice.** The DCC lends an envelope — time per device, memory — with any
  row a policy pins or allows; the agent chooses the row that fits
  ([Families and levels](../concepts/families-and-levels.md#choosing-a-row-within-the-envelope)).
- **The layer.** The chosen row runs as a layer at the family's place. The
  scheduler runs it every frame without the agent, and wakes the agent only for an
  event: a new envelope, a price that no longer fits, a failed lane, a refused
  input. A new layer is prepared off the frame, beside the installed one, and
  swapped in at a frame start.
- **The report.** After each run the DCC reads a `Report`: the row that ran, its
  measured time, how much was done, the health, whether it stalled, the lane's
  message.
- **The contention.** What the family touches follows from its lanes' typed inputs
  and outputs. Families share a concurrent wave only when their contentions are
  disjoint and neither comes `after` the other
  ([What an agent declares](../concepts/agents-and-lanes.md#what-an-agent-declares)).

## Step 7 — Switch it off and on

A family starts on. To switch it off and on while the engine runs, call
`deactivate(id)` and `activate(id)` on the `Arc<Activation>` resource with the
family's `AgentId`; the switch lands at the next frame boundary. An app does this
in `EngineApp::before_frame` to have it hold for the whole frame.

## Step 8 — Verify it works

The lanes' unit tests go beside them; their integration tests in
`crates/khora-lanes/tests/it/<family>/`. The family end to end goes in
`crates/khora-agents/tests/it/families/<family>/`: find its registration in
`registered_agents()` by `TypeId::of::<YourFamily>()`, build its agent with
`lane_agent`, and run a frame with `run_frame`, without the scheduler. From
`crates/khora-agents/tests/it/families/audio/mod.rs`, simplified:

```rust
use std::sync::Arc;

use khora_agents::lane_agent::{lane_agent, run_frame};
use khora_core::agent::Agent;
use khora_core::gpu::DeviceCaps;
use khora_core::lane::io::FrameId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::EngineContext;
use khora_data::flow::AudioPlaybackWriteback;

// `counting_mix`, `runtime_with`, `audio_registration` and `view_of_sources`
// are helpers of that test file: a mix bus that counts its blocks, a runtime
// holding it, the family's registration found by its `TypeId`, and an
// `AudioView` of three sources.
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

`run_frame` answers the first-frame wake, installs the default row and runs it, so
the family's outputs are in the deck after one call. Then:

```bash
cargo xtask lint-file <every .rs touched>
cargo t -p khora-lanes
cargo t -p khora-agents
cargo t -p xtask      # the family folder, registration and test-layout checks
cargo gate
cargo run -p sandbox  # a render family: a clean frame, no wgpu/Vulkan validation error
```

## Related

- [Families and levels](../concepts/families-and-levels.md) — levels, rows, the
  registry line, pins.
- [Agents and Lanes](../concepts/agents-and-lanes.md) — places, layers, waves.
- [GORNA](../concepts/gorna.md) — how envelopes are lent.
- [Add a lane](./add-a-lane.md) — the lanes a family chooses between.
- [Control GORNA adaptation](./control-gorna-adaptation.md) — pin or bound a
  family's row from host code.
