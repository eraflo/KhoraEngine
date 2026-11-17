# Add a lane

A lane is one way of doing a family's job: a new lighting technique for the
renderer, another mixing strategy for audio. This guide shows you how to
**implement a `Lane`** — the work, at one or more levels — declare the inputs it
reads and the outputs it writes, and add it to its family's registration so the
family's agent can choose it. The lane never decides when it runs or at which
level: its agent does, within the envelope the DCC lends
([Families and levels](../concepts/families-and-levels.md)).

**Prerequisites:** you have completed
[Extending the engine](../tutorials/extending-the-engine.md) and read
[Agents and Lanes](../concepts/agents-and-lanes.md).

A lane implements `khora_core::lane::Lane` (`crates/khora-core/src/lane/contract/lane.rs`):

| Item | What it is |
|---|---|
| `type Family` | The family whose outputs the lane writes. |
| `type Inputs<'f>` | What the lane reads each frame, a struct deriving `LaneInputs`. |
| `levels` | The levels the lane offers, cheapest first. Each is a row of the family's menu. |
| `supports` | Whether the device can run the lane at all (default: yes). A lane that cannot is left off the menu at boot. |
| `prepare` | Builds what a level needs on the device. Runs for a new layer, and again when the level or the target's formats change (default: nothing). |
| `cost` | What running a level on this frame's inputs costs. |
| `record` | Does one frame's work at a level and writes the family's outputs. |
| `shutdown` | Releases what `prepare` built (default: nothing). |

The lane never sees the `World`, the whole deck or the whole runtime — only the
fields its inputs name. The examples below come from the audio family's
`SpatialMixingLane` (`crates/khora-lanes/src/audio/spatial_mixing.rs`) and
the `SkyboxLane` (`crates/khora-lanes/src/skybox/lane.rs`).

## Step 0 — Place the file

A lane lives in its family's folder: `crates/khora-lanes/src/<family>/<lane>.rs`,
or a folder `<lane>/` when it needs several files (as `render/forward_plus/`).
Declare it in the family's `mod.rs`. A lane of a new family starts with the family:
see [Add a subsystem (a family)](./add-a-family.md).

## Step 1 — Declare the levels

A level is one set of parameters the lane runs with: a `StrategyId`, unique within
the family, a name, its `impact` on the result (`None`, `Minor`, `Major`) and the
memory it holds on each device. The mixing lane offers three, cheapest first:

```rust
use khora_core::agent::gorna::StrategyId;
use khora_core::lane::cost::PerDevice;
use khora_core::lane::level::{Impact, Level};

/// Builds a level of the lane, which holds no memory: it differs by the voices it mixes.
const fn voices(id: StrategyId, name: &'static str, impact: Impact) -> Level {
    Level {
        id,
        name,
        impact,
        memory: PerDevice { cpu: 0, gpu: 0, npu: 0, other: 0 },
    }
}

const LEVELS: [Level; 3] = [
    voices(StrategyId::LowPower, "8 voices", Impact::Minor),
    voices(StrategyId::Balanced, "32 voices", Impact::Minor),
    voices(StrategyId::HighPerformance, "128 voices", Impact::None),
];
const VOICE_CAPS: [usize; 3] = [8, 32, 128];
```

The lane maps the level's index to its parameters (`VOICE_CAPS` here). In an
`Exact` family every level must have `Impact::None`: boot refuses one that changes
the result.

## Step 2 — Declare the inputs

The inputs are a struct deriving `LaneInputs`. Each field names where it comes
from, and the derive turns those names into the lane's declaration: what it reads
and locks, from which its agent's contention follows
([What an agent declares](../concepts/agents-and-lanes.md#what-an-agent-declares)).
The mixing lane reads the View the `AudioFlow` published and the mix bus the backend
drains:

```rust
use std::sync::Arc;

use khora_core::audio::AudioMixBus;
use khora_data::flow::AudioView;
use khora_macros::LaneInputs;

#[derive(LaneInputs)]
pub struct SpatialMixingInputs<'f> {
    /// The listener and every source, snapshotted for this frame.
    #[bus]
    pub view: &'f AudioView,
    /// The bus the mixed block is pushed into.
    #[resource]
    pub mix_bus: &'f Arc<dyn AudioMixBus>,
}
```

| Attribute | Field type | What it reads |
|---|---|---|
| `#[bus]` | `&'f V` | A View a `Flow` published on the `LaneBus`. |
| `#[frame]` | `Target`, `SceneTarget`, `ClearColor` or `FixedStep` | The frame's target (`SceneTarget` refuses one without depth), its clear colour, or the family's fixed step. |
| `#[deck]` | `Option<&'f T>` | A deck slot an earlier phase may have written. |
| `#[resident]` | `&'f Resident<R>` | What a residency made usable. |
| `#[resource]` | `&'f T` | A runtime entry, read without locking. |
| `#[locked]` | `Locked<'f, T>` | A runtime entry registered as `Arc<Mutex<T>>`, locked for the record. |
| `#[budget]` | `Budget` or `Option<Budget>` | The time lent to the lane; `Option` for a lane that may run before anything is lent. |
| `#[take]` | `Vec<T>` | The requests of the family's `Take` mailbox on the runtime's `Channel<T>`, drained. |
| `#[scope]` | — | Reserved for a parallel scope; the derive refuses it today. |

`#[bus(validate)]` and `#[deck(validate)]` also run the value's `Validate` check.
A field of the wrong type fails to compile, on that field, with the attribute's
own message. A lane of a `BACKGROUND` family may read only `#[take]`,
`#[resource]` and `#[budget]`: it runs over several frames.

**Read only what you use.** Every `#[resource]` keeps a family that locks that
entry out of your wave; every `#[locked]` keeps out every family that reads it.
Nothing fails when you declare too much: the frame is just slower.

> A lane never queries the World: it reads the View a `Flow` published — see
> [Add a flow](./add-a-flow.md). Boot refuses a lane that reads a resource the
> runtime does not hold, a deck slot no earlier phase writes, or a `FixedStep` its
> family does not have.

## Step 3 — Implement `Lane`

`cost` prices a level on the frame's inputs, per device; the agent offers these
prices to GORNA and chooses the row that fits its envelope. `record` does the work
at the level it is given and writes the family's outputs. The lane records through
`&mut self`, so it may keep buffers from frame to frame. Simplified from
`spatial_mixing.rs`:

```rust
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::{Lane, LaneError, LaneReport};

use super::family::{AudioFamily, AudioOutputs};

#[derive(Default)]
pub struct SpatialMixingLane {
    /// The block mixed this frame, kept so a steady frame allocates nothing.
    block: Vec<f32>,
}

impl Lane for SpatialMixingLane {
    type Family = AudioFamily;
    type Inputs<'f> = SpatialMixingInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    /// Every playing source costs a mixed voice up to the level's cap, and a
    /// virtual one beyond it — all on the host.
    fn cost(&self, level: usize, inputs: &SpatialMixingInputs<'_>) -> Cost {
        let playing = inputs.view.sources.iter().filter(|source| plays(source)).count();
        let cap = VOICE_CAPS.get(level).copied().unwrap_or(0);
        let mixed = playing.min(cap) as u32;
        let virtual_voices = playing.saturating_sub(cap) as u32;
        Cost {
            time: PerDevice {
                cpu: BLOCK + MIXED_VOICE * mixed + VIRTUAL_VOICE * virtual_voices,
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut SpatialMixingInputs<'_>,
        out: &mut AudioOutputs,
    ) -> Result<LaneReport, LaneError> {
        let cap = VOICE_CAPS.get(level).copied().ok_or(LaneError::InvalidInput {
            expected: "a voice level the lane offers",
            received: format!("level {level}"),
        })?;
        // … mix the `cap` most audible sources of `inputs.view` into
        // `self.block`, and push one playback update per source into
        // `out.playback.updates` …
        inputs.mix_bus.write_block(&self.block);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}
```

`LaneReport` says how much of the frame's work was done, whether the lane could
not progress, and an optional message; the agent's GORNA report carries them. An
`Err` never crashes the frame: it wakes the agent, which falls back to a cheaper
row.

`cost` must be cheap and honest: it runs every frame for every level of the lane
that runs, and the agent chooses on it. A level that serves only part of the
frame's work says so in `Cost::share`; one whose impact depends on the inputs says
so in `Cost::impact`.

## Step 4 — Build device resources in `prepare`

A lane that draws builds its buffers and pipelines in `prepare`, for the level and
the target's formats (`TargetKey`), and releases them in `shutdown`. `prepare` runs
off the frame for a new layer, while the old one keeps drawing, and again on a run
when the level or the target's formats change. It resolves a pipeline by name
through the `PipelineSystem`, never by handing over WGSL source. Simplified from the
`SkyboxLane`:

```rust
use khora_core::gpu::GpuSetup;
use khora_core::lane::io::TargetKey;

impl Lane for SkyboxLane {
    // … `Family`, `Inputs`, `levels`, `cost`, `record` …

    fn prepare(
        &mut self,
        _level: usize,
        gpu: &mut GpuSetup<'_>,
        key: Option<TargetKey>,
    ) -> Result<(), LaneError> {
        let device = gpu.device().ok_or(LaneError::missing("GraphicsDevice"))?;
        let pipelines = gpu.pipelines().ok_or(LaneError::missing("PipelineSystem"))?;
        // Build the uniform buffer once; build a pipeline for `key`'s formats
        // whenever the target changes:
        // `pipelines.pipeline(device, &skybox_pipeline_spec(key))`, whose spec
        // names the shader `khora::pipelines::skybox`.
        Ok(())
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(sky), Some(device)) = (self.gpu.take(), gpu.device()) {
            destroy_sky_gpu(sky, device);
        }
    }
}
```

Its `record` reads the frame's `#[frame] target: Target` and writes the family's
`#[pass]` output: `out.pass.device()` for buffer writes, `out.pass.encoder()` to
record the draw. See [Add a shader](./add-a-shader.md) for the pipeline itself.

## Step 5 — Write the outputs

A lane writes only its family's outputs, the struct deriving `LaneOutputs` named by
`LaneFamily::Outputs`. It receives them opened for the frame (a `#[deck]` field
holds the slot's value this frame, or its default), and they are committed to the
deck after `record` returns. Every lane of a family writes the same outputs, which
is what lets one stand in for another. To write something new, add a field to the
family's outputs; to start a new family, see
[Add a subsystem (a family)](./add-a-family.md).

## Step 6 — Add the lane to its family's registration

List the lane in its family's `register_agent!`. The render family's three lanes,
from `crates/khora-lanes/src/render/registration.rs`:

```rust
lanes: [SimpleUnlitLane, LitForwardLane, ForwardPlusLane],
```

Every level of every lane the device supports becomes a row of the agent's menu.
The lane must name that family as its `Lane::Family`, or the registration does not
compile; its `StrategyId`s must differ from those of the family's other lanes, or
boot refuses the family. The registration builds the lane with `Default`.

## Step 7 — Verify it works

Unit tests go beside the lane; its integration tests in
`crates/khora-lanes/tests/it/<family>/`. Drive the whole family through one frame
with `khora_agents::lane_agent::run_frame`, in
`crates/khora-agents/tests/it/families/<family>/`, as in
[Add a subsystem (a family)](./add-a-family.md#step-8--verify-it-works), and assert
on what the lane wrote into the deck. Then:

```bash
cargo xtask lint-file <every .rs touched>
cargo t -p khora-lanes
cargo t -p khora-agents
cargo gate
cargo run -p sandbox   # a render lane: a clean frame, no wgpu/Vulkan validation error
```

## Related

- [Agents and Lanes](../concepts/agents-and-lanes.md) — families, layers, the bus and the deck.
- [Families and levels](../concepts/families-and-levels.md) — levels, rows and how one is chosen.
- [Add a flow](./add-a-flow.md) — produce the View this lane consumes.
- [Add a subsystem (a family)](./add-a-family.md) — the family and its registration.
