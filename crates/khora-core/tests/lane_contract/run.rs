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

//! One run of an erased lane: the level checked, the inputs fetched, the lane
//! prepared when its target changes, recorded, every level costed, the outputs
//! committed.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::erased::{AnyLane, Erased, Run, RunOutcome};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{InputRefusal, Reason, SourceKind};
use khora_core::lane::io::{FrameId, InputSource, OutputSink, Target, TargetKey};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneBus, LaneError, LaneKind, OutputDeck};
use khora_core::math::Extent2D;
use khora_core::renderer::api::resource::{TextureFormat, TextureViewId};
use khora_core::Runtime;
use khora_macros::{LaneInputs, LaneOutputs};

use crate::fixtures::level;

/// A slot the draw family writes: how many records wrote it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Drawn(pub u32);

/// What the draw lane reads: the frame's target.
#[derive(LaneInputs)]
pub struct DrawInputs {
    #[frame]
    pub target: Target,
}

/// The draw family's outputs.
#[derive(LaneOutputs)]
pub struct DrawOutputs {
    #[deck]
    pub drawn: Drawn,
}

/// Draws into the frame's target.
pub struct DrawFamily;

impl LaneFamily for DrawFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "draw";
    const AGENT: AgentId = AgentId::Renderer;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = DrawOutputs;
}

/// What the draw lane was asked to do, in order.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Prepare(Option<TargetKey>),
    Record(usize),
}

const DRAW_LEVELS: &[Level] = &[
    level(StrategyId::LowPower, "cheap"),
    level(StrategyId::Balanced, "full"),
];

/// Logs every prepare and record; fails its record when told to.
struct DrawLane {
    log: Arc<Mutex<Vec<Event>>>,
    fail_record: bool,
}

impl Lane for DrawLane {
    type Family = DrawFamily;
    type Inputs<'f> = DrawInputs;

    fn levels(&self) -> &[Level] {
        DRAW_LEVELS
    }

    fn prepare(
        &mut self,
        _gpu: &mut GpuSetup<'_>,
        key: Option<TargetKey>,
    ) -> Result<(), LaneError> {
        self.log.lock().unwrap().push(Event::Prepare(key));
        Ok(())
    }

    fn cost(&self, level: usize, inputs: &DrawInputs) -> Cost {
        let pixels = u64::from(inputs.target.size.width) * u64::from(inputs.target.size.height);
        Cost {
            memory: PerDevice {
                gpu: pixels * (level as u64 + 1),
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut DrawInputs,
        out: &mut DrawOutputs,
    ) -> Result<LaneReport, LaneError> {
        self.log.lock().unwrap().push(Event::Record(level));
        if self.fail_record {
            return Err(LaneError::InvalidContext {
                expected: "a record that works",
                received: "a lane told to fail".to_owned(),
            });
        }
        out.drawn = Drawn(1);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

fn key(color: TextureFormat) -> TargetKey {
    TargetKey {
        color,
        depth: Some(TextureFormat::Depth32Float),
        samples: 1,
    }
}

fn target(key: TargetKey, width: u32, height: u32) -> Target {
    Target {
        color: TextureViewId(1),
        depth: Some(TextureViewId(2)),
        size: Extent2D { width, height },
        key,
    }
}

fn draw_lane(fail_record: bool) -> (Erased<DrawLane>, Arc<Mutex<Vec<Event>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let lane = Erased::new(DrawLane {
        log: Arc::clone(&log),
        fail_record,
    });
    (lane, log)
}

/// Runs `lane` once at `level` into `target`, committing into `deck`.
fn run_into(
    lane: &mut Erased<DrawLane>,
    level: usize,
    target: Target,
    deck: &mut OutputDeck,
) -> Run {
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let mut source = InputSource::new(FrameId(1), &bus, &runtime)
        .with_target(target)
        .with_budget(khora_core::lane::io::Budget {
            time: Duration::from_millis(1),
        });
    let mut sink = OutputSink::new(deck);
    let mut gpu = GpuSetup::headless();
    lane.run(level, &mut source, &mut sink, &mut gpu)
}

/// A lane builds its pipelines for its target's formats: it is prepared
/// before its first record, not again while the target keeps its key, and
/// again each time the key changes — back to an earlier key included.
#[test]
fn prepare_runs_before_the_first_record_and_on_target_change() {
    let (mut lane, log) = draw_lane(false);
    let mut deck = OutputDeck::new();
    let srgb = key(TextureFormat::Rgba8UnormSrgb);
    let hdr = key(TextureFormat::Rgba16Float);

    for key in [srgb, srgb, hdr, hdr, srgb] {
        let run = run_into(&mut lane, 1, target(key, 4, 4), &mut deck);
        assert!(
            matches!(run.outcome, RunOutcome::Recorded(_)),
            "{:?}",
            run.outcome
        );
    }

    assert_eq!(
        *log.lock().unwrap(),
        vec![
            Event::Prepare(Some(srgb)),
            Event::Record(1),
            Event::Record(1),
            Event::Prepare(Some(hdr)),
            Event::Record(1),
            Event::Record(1),
            Event::Prepare(Some(srgb)),
            Event::Record(1),
        ]
    );
}

/// GORNA prices every row from the frame the lane just ran on: one cost per
/// level, in level order, computed on this frame's inputs.
#[test]
fn every_level_is_costed_on_the_frame_inputs() {
    let (mut lane, _log) = draw_lane(false);
    let mut deck = OutputDeck::new();

    let run = run_into(
        &mut lane,
        0,
        target(key(TextureFormat::Rgba8Unorm), 2, 3),
        &mut deck,
    );

    let gpu_memory: Vec<u64> = run.costs.iter().map(|cost| cost.memory.gpu).collect();
    assert_eq!(gpu_memory, vec![6, 12], "both levels, cheapest first");
    assert_eq!(deck.get::<Drawn>(), Some(&Drawn(1)));
}

/// A level the lane does not offer is a failure of the run itself: nothing
/// is fetched (no lock taken), nothing prepared, nothing recorded.
#[test]
fn a_level_the_lane_does_not_offer_fails_before_any_fetch() {
    let (mut lane, log) = draw_lane(false);
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let mut deck = OutputDeck::new();
    let mut source = InputSource::new(FrameId(1), &bus, &runtime).with_target(target(
        key(TextureFormat::Rgba8Unorm),
        4,
        4,
    ));
    let mut gpu = GpuSetup::headless();

    let run = {
        let mut sink = OutputSink::new(&mut deck);
        lane.run(DRAW_LEVELS.len(), &mut source, &mut sink, &mut gpu)
    };

    assert!(
        matches!(
            run.outcome,
            RunOutcome::Failed(LaneError::InvalidContext { .. })
        ),
        "{:?}",
        run.outcome
    );
    assert!(run.costs.is_empty());
    assert!(
        source.touched().inputs().is_empty(),
        "nothing was fetched: {:?}",
        source.touched()
    );
    assert!(
        log.lock().unwrap().is_empty(),
        "nothing prepared or recorded"
    );
    assert_eq!(deck.get::<Drawn>(), None, "nothing committed");
}

/// A lane that fails its record commits nothing: the slot keeps what it held,
/// and every level is still costed for GORNA.
#[test]
fn a_lane_error_commits_no_output() {
    let (mut lane, _log) = draw_lane(true);
    let mut deck = OutputDeck::new();
    *deck.slot::<Drawn>() = Drawn(7);

    let run = run_into(
        &mut lane,
        0,
        target(key(TextureFormat::Rgba8Unorm), 4, 4),
        &mut deck,
    );

    assert!(
        matches!(run.outcome, RunOutcome::Failed(_)),
        "{:?}",
        run.outcome
    );
    assert_eq!(run.costs.len(), DRAW_LEVELS.len());
    assert_eq!(
        deck.get::<Drawn>(),
        Some(&Drawn(7)),
        "the failed record's default output was not committed over the slot"
    );
}

/// A target with no size is refused before the lane is prepared for it.
#[test]
fn a_target_without_size_is_refused_before_prepare() {
    let (mut lane, log) = draw_lane(false);
    let mut deck = OutputDeck::new();

    let run = run_into(
        &mut lane,
        0,
        target(key(TextureFormat::Rgba8Unorm), 0, 4),
        &mut deck,
    );

    match run.outcome {
        RunOutcome::Refused(refusal) => assert_eq!(
            refusal,
            InputRefusal {
                field: "target",
                source: SourceKind::Frame,
                reason: Reason::BadTarget,
            }
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(log.lock().unwrap().is_empty());
}
