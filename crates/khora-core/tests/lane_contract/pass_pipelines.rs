// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! What a pass lends the lane recording into it beside the device: the
//! pipeline system the agent lent the sink, so a lane resolves the pipeline a
//! draw needs while it records — no pipeline system read as a runtime
//! resource, which a headless boot does not hold. And a family whose outputs
//! are two pass slots of different kinds opens and closes both, each into its
//! own slot.

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased, LaneRun, RunOutcome};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::{FrameId, InputSource, OutputSink, Pass, PassSlot};
use khora_core::lane::level::Level;
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneBus, LaneError, OutputDeck};
use khora_core::renderer::api::command::{
    BindGroupLayoutEntry, BindGroupLayoutId, CommandBufferId, RenderPassDescriptor,
};
use khora_core::renderer::api::pipeline::{
    ComputePipelineId, ComputePipelineSpec, LayoutKey, PipelineSpec, RenderPipelineId,
};
use khora_core::renderer::api::shader::ShaderVariantKey;
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_macros::{LaneInputs, LaneOutputs};

use crate::fixtures::level;
use crate::pass_device::EncoderLedger;
use crate::passes::DrawnPass;

// ─── A pipeline system to lend ──────────────────────────────────────────────

/// A pipeline system no test asks anything of: it is lent, and recognised by
/// its address.
#[derive(Debug, Default)]
pub struct LentPipelines;

const LENT_ONLY: &str = "the pipeline system is only lent here, never asked";

impl PipelineSystem for LentPipelines {
    fn layout(
        &self,
        _device: &dyn GraphicsDevice,
        _key: LayoutKey,
        _variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError> {
        unreachable!("{LENT_ONLY}")
    }
    fn inline_layout(
        &self,
        _device: &dyn GraphicsDevice,
        _label: &'static str,
        _entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError> {
        unreachable!("{LENT_ONLY}")
    }
    fn pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError> {
        unreachable!("{LENT_ONLY}")
    }
    fn compute_pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError> {
        unreachable!("{LENT_ONLY}")
    }
}

/// The address a pipeline system is known by.
fn address(pipelines: &dyn PipelineSystem) -> usize {
    pipelines as *const dyn PipelineSystem as *const () as usize
}

// ─── A family of two passes ─────────────────────────────────────────────────

/// A second kind of pass slot, as the frame graph's scene and transparent
/// slots are two: empty, or holding its descriptor's names and the buffer.
#[derive(Debug, Default, PartialEq)]
pub struct OverPass(pub Option<(Vec<&'static str>, CommandBufferId)>);

impl PassSlot for OverPass {
    type Descriptor = Vec<&'static str>;

    fn descriptor() -> Vec<&'static str> {
        vec!["over"]
    }

    fn close(descriptor: Vec<&'static str>, command_buffer: CommandBufferId) -> Self {
        OverPass(Some((descriptor, command_buffer)))
    }
}

/// What each pass of a record was lent: the address of the pipeline system,
/// or `None`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LentToPasses(pub Vec<Option<usize>>);

/// The two-pass family's outputs: two passes of different slots, and what
/// each was lent.
#[derive(LaneOutputs)]
pub struct TwoSlotOutputs<'f> {
    #[pass]
    pub first: Pass<'f, DrawnPass>,
    #[pass]
    pub over: Pass<'f, OverPass>,
    #[deck]
    pub lent: LentToPasses,
}

/// A family recording two passes a frame, as the render family records its
/// opaque and its blended draws.
pub struct TwoPassFamily;

impl LaneFamily for TwoPassFamily {
    const NAME: &'static str = "two passes";
    const AGENT: AgentId = AgentId::new("renderer");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = TwoSlotOutputs<'f>;
}

/// The two-pass lane reads nothing.
#[derive(LaneInputs)]
pub struct NoInputs {}

const TWO_PASS_LEVELS: &[Level] = &[level(StrategyId::Balanced, "two passes")];

/// Draws once into each pass, names each, and writes down what each pass was
/// lent.
#[derive(Default)]
pub struct TwoPassLane;

impl Lane for TwoPassLane {
    type Family = TwoPassFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        TWO_PASS_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        out: &mut TwoSlotOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        for (encoder, label) in [(out.first.encoder(), "first"), (out.over.encoder(), "over")] {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[],
                depth_stencil_attachment: None,
            });
            pass.draw(0..3, 0..1);
        }
        out.first.descriptor_mut().push("first drawn");
        out.over.descriptor_mut().push("over drawn");
        out.lent = LentToPasses(vec![
            out.first.pipelines().map(address),
            out.over.pipelines().map(address),
        ]);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

/// Runs the two-pass lane once into `deck`, its passes opening on `device`,
/// the sink lent `pipelines` when there are some.
fn run_two_passes(
    device: &EncoderLedger,
    pipelines: Option<&dyn PipelineSystem>,
    deck: &mut OutputDeck,
) -> LaneRun {
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let mut inputs = InputSource::new(FrameId(0), &bus, &runtime);
    let mut outputs = OutputSink::new(deck).with_device(device);
    if let Some(pipelines) = pipelines {
        outputs = outputs.with_pipelines(pipelines);
    }
    let mut gpu = GpuSetup::new(device);
    Erased::new(TwoPassLane).run(0, &mut inputs, &mut outputs, &mut gpu)
}

// ─── The pipelines a pass lends ─────────────────────────────────────────────

/// **A pass lends the pipeline system its sink was lent.** The agent lends
/// the sink the run's pipeline system beside the device; every pass the
/// outputs open hands the lane that very system — the same object, not a
/// copy — so a draw's pipeline is resolved while it is recorded.
#[test]
fn a_pass_lends_the_pipeline_system_its_sink_was_lent() {
    let device = EncoderLedger::new();
    let pipelines = LentPipelines;
    let mut deck = OutputDeck::new();

    let run = run_two_passes(&device, Some(&pipelines), &mut deck);

    assert!(
        matches!(run.outcome, RunOutcome::Recorded(_)),
        "the lane recorded: {:?}",
        run.outcome
    );
    let ours = Some(address(&pipelines));
    assert_eq!(
        deck.get::<LentToPasses>(),
        Some(&LentToPasses(vec![ours, ours])),
        "both passes lent the sink's pipeline system"
    );
}

/// **A pass of a sink lent no pipeline system lends none.** A sink lent only
/// a device opens its passes on it, and each pass says it has no pipeline
/// system rather than inventing one.
#[test]
fn a_pass_of_a_sink_lent_no_pipelines_lends_none() {
    let device = EncoderLedger::new();
    let mut deck = OutputDeck::new();

    let run = run_two_passes(&device, None, &mut deck);

    assert!(
        matches!(run.outcome, RunOutcome::Recorded(_)),
        "a pass needs no pipeline system to record: {:?}",
        run.outcome
    );
    assert_eq!(
        deck.get::<LentToPasses>(),
        Some(&LentToPasses(vec![None, None])),
        "no pipeline system was lent, so no pass has one"
    );
}

// ─── Two pass slots ─────────────────────────────────────────────────────────

/// Draws once into each of the two passes; lends nothing to look at.
#[derive(Default)]
pub struct TwoDrawsLane;

/// The two-draw family's outputs: two passes of different slots.
#[derive(LaneOutputs)]
pub struct TwoDraws<'f> {
    #[pass]
    pub first: Pass<'f, DrawnPass>,
    #[pass]
    pub over: Pass<'f, OverPass>,
}

/// A family of two pass slots and nothing else.
pub struct TwoDrawsFamily;

impl LaneFamily for TwoDrawsFamily {
    const NAME: &'static str = "two draws";
    const AGENT: AgentId = AgentId::new("renderer");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = TwoDraws<'f>;
}

impl Lane for TwoDrawsLane {
    type Family = TwoDrawsFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        TWO_PASS_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        out: &mut TwoDraws<'_>,
    ) -> Result<LaneReport, LaneError> {
        for encoder in [out.first.encoder(), out.over.encoder()] {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: None,
            });
            pass.draw(0..3, 0..1);
        }
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

/// **A family of two pass slots opens and closes both.** Each pass opens an
/// encoder of its own on the sink's device, in declaration order; the lane
/// draws through each; on commit each encoder is finished into its own slot,
/// with its own descriptor — the opaque scene and the blended draws are two
/// command buffers the frame graph orders apart. Nothing is submitted.
#[test]
fn a_family_of_two_pass_slots_opens_and_closes_both() {
    let device = EncoderLedger::new();
    let mut deck = OutputDeck::new();
    let run = {
        let bus = LaneBus::new();
        let runtime = Runtime::new();
        let mut inputs = InputSource::new(FrameId(0), &bus, &runtime);
        let mut outputs = OutputSink::new(&mut deck).with_device(&device);
        let mut gpu = GpuSetup::new(&device);
        Erased::new(TwoDrawsLane).run(0, &mut inputs, &mut outputs, &mut gpu)
    };

    assert!(
        matches!(run.outcome, RunOutcome::Recorded(_)),
        "the lane recorded: {:?}",
        run.outcome
    );
    let opened = device.opened();
    assert_eq!(opened.len(), 2, "one encoder per pass: {opened:?}");
    assert_eq!(
        device.draws(),
        opened,
        "one draw through each pass's encoder"
    );
    let mut finished = device.finished();
    finished.sort_by_key(|buffer| buffer.0);
    assert_eq!(finished, opened, "both encoders were finished");
    assert_eq!(
        deck.get::<DrawnPass>(),
        Some(&DrawnPass(Some((vec!["drawn"], opened[0])))),
        "the first pass closed into its slot"
    );
    assert_eq!(
        deck.get::<OverPass>(),
        Some(&OverPass(Some((vec!["over"], opened[1])))),
        "the second pass closed into its own slot"
    );
    assert!(device.submitted().is_empty(), "a lane submits nothing");
}
