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

//! What a `#[pass]` output is: an encoder opened on the sink's device, which
//! the lane records into, closed on commit into its pass slot with the buffer
//! the encoder finished into. A pass that cannot be closed is refused, and a
//! lane that fails leaves no half pass behind.

use std::any::TypeId;

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased, Run, RunOutcome};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{OutputRefusal, Reason, SinkKind};
use khora_core::lane::io::{FrameId, InputSource, LaneOutputs, OutputSink, Pass, PassSlot};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneBus, LaneError, LaneKind, OutputDeck};
use khora_core::renderer::api::command::{CommandBufferId, RenderPassDescriptor};
use khora_core::Runtime;
use khora_macros::{LaneInputs, LaneOutputs};

use crate::fixtures::{level, Echoes};
use crate::pass_device::EncoderLedger;

// ─── A pass slot, and a family recording into it ────────────────────────────

/// A pass slot as the frame graph's are: empty, or holding one closed pass —
/// here, the names its descriptor gathered and the finished buffer.
#[derive(Debug, Default, PartialEq)]
pub struct DrawnPass(pub Option<(Vec<&'static str>, CommandBufferId)>);

impl PassSlot for DrawnPass {
    type Descriptor = Vec<&'static str>;

    fn descriptor() -> Vec<&'static str> {
        vec!["drawn"]
    }

    fn close(descriptor: Vec<&'static str>, command_buffer: CommandBufferId) -> Self {
        DrawnPass(Some((descriptor, command_buffer)))
    }
}

/// The draw family's outputs: one pass, and a deck slot beside it.
#[derive(LaneOutputs)]
pub struct DrawOutputs<'f> {
    #[pass]
    pub pass: Pass<'f, DrawnPass>,
    #[deck]
    pub echoes: Echoes,
}

/// A family recording one pass a frame.
pub struct PassFamily;

impl LaneFamily for PassFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "pass";
    const AGENT: AgentId = AgentId::Renderer;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = DrawOutputs<'f>;
}

/// The pass lanes read nothing.
#[derive(LaneInputs)]
pub struct Nothing {}

const PASS_LEVELS: &[Level] = &[level(StrategyId::Balanced, "one pass")];

/// Records one draw into its pass, names the pass in the descriptor, writes
/// its deck slot — and then fails, when told to.
#[derive(Default)]
pub struct DrawLane {
    fails: bool,
}

impl Lane for DrawLane {
    type Family = PassFamily;
    type Inputs<'f> = Nothing;

    fn levels(&self) -> &[Level] {
        PASS_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &Nothing) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut Nothing,
        out: &mut DrawOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        {
            let mut pass = out.pass.encoder().begin_render_pass(&RenderPassDescriptor {
                label: Some("draw"),
                color_attachments: &[],
                depth_stencil_attachment: None,
            });
            pass.draw(0..3, 0..1);
        }
        out.pass.descriptor_mut().push("amended");
        out.echoes = Echoes(1);
        if self.fails {
            return Err(LaneError::MissingResource {
                kind: "test",
                key: "a lane failing after it drew".to_owned(),
            });
        }
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

/// Runs `lane` once into `deck`, its passes opening on `device`.
fn run_on(device: &EncoderLedger, lane: DrawLane, deck: &mut OutputDeck) -> Run {
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let mut inputs = InputSource::new(FrameId(0), &bus, &runtime);
    let mut outputs = OutputSink::new(deck).with_device(device);
    let mut gpu = GpuSetup::new(device);
    Erased::new(lane).run(0, &mut inputs, &mut outputs, &mut gpu)
}

// ─── Closing a pass ─────────────────────────────────────────────────────────

/// **A pass closes into its slot.** The lane records into the encoder its
/// `#[pass]` output opened on the sink's device; on commit the encoder is
/// finished and the slot holds the pass, with the descriptor the lane amended
/// and the very buffer the encoder finished into. Nothing is submitted: the
/// engine submits passes, in the frame graph's order.
#[test]
fn a_pass_output_closes_into_its_slot_with_the_buffer_its_encoder_finished() {
    let device = EncoderLedger::new();
    let mut deck = OutputDeck::new();

    let run = run_on(&device, DrawLane::default(), &mut deck);

    assert!(
        matches!(run.outcome, RunOutcome::Recorded(_)),
        "the lane recorded: {:?}",
        run.outcome
    );
    let opened = device.opened();
    assert_eq!(opened.len(), 1, "one encoder for the one pass: {opened:?}");
    assert_eq!(
        device.draws(),
        vec![opened[0]],
        "the lane drew through the pass's encoder"
    );
    assert_eq!(
        device.finished(),
        vec![opened[0]],
        "the commit finished the encoder"
    );
    assert_eq!(
        deck.get::<DrawnPass>(),
        Some(&DrawnPass(Some((vec!["drawn", "amended"], opened[0])))),
        "the slot holds the closed pass, its descriptor as the lane left it"
    );
    assert_eq!(
        deck.get::<Echoes>(),
        Some(&Echoes(1)),
        "the deck slot beside"
    );
    assert!(
        device.submitted().is_empty(),
        "a lane submits nothing: {:?}",
        device.submitted()
    );
}

/// **A pass that cannot be closed is refused.** An encoder that does not
/// finish gives no buffer to submit; the commit refuses the output as not
/// closed, naming the field, and the slot is left empty rather than holding a
/// pass the engine could not submit.
#[test]
fn a_pass_whose_encoder_does_not_finish_is_refused_as_not_closed() {
    let device = EncoderLedger::unfinishable();
    let mut deck = OutputDeck::new();

    let run = run_on(&device, DrawLane::default(), &mut deck);

    match run.outcome {
        RunOutcome::OutputRefused(refusal) => assert_eq!(
            refusal,
            OutputRefusal {
                field: "pass",
                sink: SinkKind::Pass,
                reason: Reason::PassNotClosed,
            }
        ),
        other => panic!("expected the pass to be refused as not closed, got {other:?}"),
    }
    assert!(
        !deck.contains::<DrawnPass>(),
        "an unclosed pass leaves its slot empty: {:?}",
        deck.get::<DrawnPass>()
    );
}

/// **A lane error leaves no half pass.** The lane drew into its pass, then
/// failed: the encoder is dropped unfinished, and neither the pass slot nor
/// the deck slot it wrote holds anything.
#[test]
fn a_lane_error_leaves_no_half_pass() {
    let device = EncoderLedger::new();
    let mut deck = OutputDeck::new();

    let run = run_on(&device, DrawLane { fails: true }, &mut deck);

    assert!(
        matches!(run.outcome, RunOutcome::Failed(_)),
        "the lane failed: {:?}",
        run.outcome
    );
    assert_eq!(device.opened().len(), 1, "the pass was opened");
    assert!(
        device.finished().is_empty(),
        "a failed record finishes no encoder: {:?}",
        device.finished()
    );
    assert!(
        !deck.contains::<DrawnPass>(),
        "no half pass in the slot: {:?}",
        deck.get::<DrawnPass>()
    );
    assert!(
        !deck.contains::<Echoes>(),
        "nothing of a failed record is committed"
    );
}

// ─── What a pass output contends for ────────────────────────────────────────

/// **A pass output contends for its slot.** A pass slot lives in the deck the
/// agent writes, so two agents writing one slot in a concurrent wave would
/// lose one pass at the merge: the slot is part of the lane's deck contention,
/// beside its `#[deck]` outputs.
#[test]
fn a_pass_output_contends_for_its_slot() {
    let lane = Erased::new(DrawLane::default());
    let declaration = lane.declaration();

    assert!(
        declaration
            .outputs()
            .iter()
            .any(|output| output.kind == SinkKind::Pass && output.ty == TypeId::of::<DrawnPass>()),
        "the pass is declared: {:?}",
        declaration.outputs()
    );
    let deck = declaration.contention().deck;
    assert!(
        deck.contains(&TypeId::of::<DrawnPass>()),
        "the pass slot is contended for: {deck:?}"
    );
    assert!(
        deck.contains(&TypeId::of::<Echoes>()),
        "and the deck slot: {deck:?}"
    );
}

// ─── One pass a slot ────────────────────────────────────────────────────────

/// Two fields closing into one pass slot.
#[derive(LaneOutputs)]
pub struct TwoPasses<'f> {
    #[pass]
    pub first: Pass<'f, DrawnPass>,
    #[pass]
    pub second: Pass<'f, DrawnPass>,
}

/// A pass slot holds one pass a frame, as a deck slot holds one value: the
/// second pass closing into it is refused, naming its field, and the first
/// stays.
#[test]
fn a_pass_slot_closed_twice_is_refused() {
    let device = EncoderLedger::new();
    let mut deck = OutputDeck::new();
    let committed = {
        let mut sink = OutputSink::new(&mut deck).with_device(&device);
        let outputs = TwoPasses::open(&mut sink).expect("both passes open on the device");
        outputs.commit(&mut sink)
    };

    assert_eq!(
        committed,
        Err(OutputRefusal {
            field: "second",
            sink: SinkKind::Pass,
            reason: Reason::WrittenTwice,
        })
    );
    let opened = device.opened();
    assert_eq!(opened.len(), 2, "each pass opened its encoder: {opened:?}");
    assert_eq!(
        deck.get::<DrawnPass>(),
        Some(&DrawnPass(Some((vec!["drawn"], opened[0])))),
        "the first pass stays in the slot"
    );
}
