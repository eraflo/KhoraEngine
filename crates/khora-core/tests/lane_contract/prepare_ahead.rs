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

//! A fresh lane prepared ahead of its first run, from the frame's inputs: what
//! an agent does for a new layer beside the installed one, so the frame the
//! layer is swapped in prepares nothing.

use std::sync::{Arc, Mutex};

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased, NotPrepared, RunOutcome};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::Reason;
use khora_core::lane::io::{FrameId, InputSource, OutputSink, Target, TargetKey};
use khora_core::lane::level::Level;
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneBus, LaneError, LaneKind, OutputDeck};
use khora_core::math::Extent2D;
use khora_core::renderer::api::resource::{TextureFormat, TextureViewId};
use khora_core::Runtime;
use khora_macros::{LaneInputs, LaneOutputs};

use crate::fixtures::level;

/// What the sketch family writes: how many records wrote it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Sketched(pub u32);

/// What the sketch lane reads: the frame's target.
#[derive(LaneInputs)]
pub struct SketchInputs {
    #[frame]
    pub target: Target,
}

/// The sketch family's outputs.
#[derive(LaneOutputs)]
pub struct SketchOutputs {
    #[deck]
    pub sketched: Sketched,
}

/// Draws a sketch into the frame's target.
pub struct SketchFamily;

impl LaneFamily for SketchFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "sketch";
    const AGENT: AgentId = AgentId::new("renderer");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = SketchOutputs;
}

/// What the sketch lane was asked to do, in order.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Prepare(usize, Option<TargetKey>),
    Record(usize),
}

const SKETCH_LEVELS: &[Level] = &[
    level(StrategyId::LowPower, "rough"),
    level(StrategyId::Balanced, "clean"),
];

/// Logs every prepare and record; fails its prepare when told to.
struct SketchLane {
    log: Arc<Mutex<Vec<Event>>>,
    fail_prepare: bool,
}

impl Lane for SketchLane {
    type Family = SketchFamily;
    type Inputs<'f> = SketchInputs;

    fn levels(&self) -> &[Level] {
        SKETCH_LEVELS
    }

    fn prepare(
        &mut self,
        level: usize,
        _gpu: &mut GpuSetup<'_>,
        key: Option<TargetKey>,
    ) -> Result<(), LaneError> {
        self.log.lock().unwrap().push(Event::Prepare(level, key));
        if self.fail_prepare {
            return Err(LaneError::InvalidInput {
                expected: "a prepare that works",
                received: "a lane told to fail it".to_owned(),
            });
        }
        Ok(())
    }

    fn cost(&self, _level: usize, _inputs: &SketchInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut SketchInputs,
        out: &mut SketchOutputs,
    ) -> Result<LaneReport, LaneError> {
        self.log.lock().unwrap().push(Event::Record(level));
        out.sketched = Sketched(1);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

fn key(color: TextureFormat) -> TargetKey {
    TargetKey {
        color,
        depth: None,
        samples: 1,
    }
}

fn target(key: TargetKey) -> Target {
    Target {
        color: TextureViewId(1),
        depth: None,
        size: Extent2D {
            width: 4,
            height: 4,
        },
        key,
    }
}

fn sketch_lane(fail_prepare: bool) -> (Erased<SketchLane>, Arc<Mutex<Vec<Event>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let lane = Erased::new(SketchLane {
        log: Arc::clone(&log),
        fail_prepare,
    });
    (lane, log)
}

/// Prepares `lane` ahead at `level`, on a frame drawing into `target` (none
/// when `None`).
fn prepare_ahead(
    lane: &mut Erased<SketchLane>,
    level: usize,
    target: Option<Target>,
) -> Result<(), NotPrepared> {
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let mut source = InputSource::new(FrameId(1), &bus, &runtime);
    if let Some(target) = target {
        source = source.with_target(target);
    }
    let mut gpu = GpuSetup::headless();
    lane.prepare(level, &mut source, &mut gpu)
}

/// Runs `lane` once at `level` into `target`.
fn run_into(lane: &mut Erased<SketchLane>, level: usize, target: Target) -> RunOutcome {
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let mut deck = OutputDeck::new();
    let mut source = InputSource::new(FrameId(1), &bus, &runtime).with_target(target);
    let mut sink = OutputSink::new(&mut deck);
    let mut gpu = GpuSetup::headless();
    lane.run(level, &mut source, &mut sink, &mut gpu).outcome
}

/// **A lane prepared ahead does not prepare on its first run.** Prepared at
/// its clean level for the frame's sRGB target, its first run on that target
/// and level records at once.
#[test]
fn a_lane_prepared_ahead_does_not_prepare_on_its_first_run() {
    let (mut lane, log) = sketch_lane(false);
    let srgb = key(TextureFormat::Rgba8UnormSrgb);

    assert!(prepare_ahead(&mut lane, 1, Some(target(srgb))).is_ok());
    let outcome = run_into(&mut lane, 1, target(srgb));

    assert!(matches!(outcome, RunOutcome::Recorded(_)), "{outcome:?}");
    assert_eq!(
        *log.lock().unwrap(),
        vec![Event::Prepare(1, Some(srgb)), Event::Record(1)],
        "prepared once, ahead"
    );
}

/// A lane prepared ahead for one target that runs on another prepares again
/// for the one it draws into: preparing ahead never stands in for a target
/// change.
#[test]
fn a_lane_prepared_ahead_prepares_again_for_another_target() {
    let (mut lane, log) = sketch_lane(false);
    let srgb = key(TextureFormat::Rgba8UnormSrgb);
    let hdr = key(TextureFormat::Rgba16Float);

    assert!(prepare_ahead(&mut lane, 0, Some(target(srgb))).is_ok());
    let _ = run_into(&mut lane, 0, target(hdr));

    assert_eq!(
        *log.lock().unwrap(),
        vec![
            Event::Prepare(0, Some(srgb)),
            Event::Prepare(0, Some(hdr)),
            Event::Record(0),
        ]
    );
}

/// **A refused input prepares nothing, and says so**: with no target on the
/// frame the lane cannot know what to prepare for. Its first run, once the
/// target is there, prepares it.
#[test]
fn preparing_ahead_without_the_inputs_prepares_nothing_and_says_why() {
    let (mut lane, log) = sketch_lane(false);
    let srgb = key(TextureFormat::Rgba8UnormSrgb);

    match prepare_ahead(&mut lane, 1, None) {
        Err(NotPrepared::Refused(refusal)) => assert_eq!(refusal.reason, Reason::Missing),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(log.lock().unwrap().is_empty(), "nothing prepared");

    let _ = run_into(&mut lane, 1, target(srgb));
    assert_eq!(
        *log.lock().unwrap(),
        vec![Event::Prepare(1, Some(srgb)), Event::Record(1)]
    );
}

/// **A prepare that fails is the lane's failure**: the agent learns it before
/// the layer is ever installed. The lane is left unprepared, so a run would
/// try again.
#[test]
fn a_prepare_that_fails_ahead_is_reported_as_a_failure() {
    let (mut lane, log) = sketch_lane(true);
    let srgb = key(TextureFormat::Rgba8UnormSrgb);

    let prepared = prepare_ahead(&mut lane, 0, Some(target(srgb)));

    assert!(
        matches!(prepared, Err(NotPrepared::Failed(_))),
        "{prepared:?}"
    );
    let _ = run_into(&mut lane, 0, target(srgb));
    assert_eq!(
        log.lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, Event::Prepare(..)))
            .count(),
        2,
        "the run tries the prepare again"
    );
}

/// A level the lane does not offer cannot be prepared: a failure, and nothing
/// is asked of the lane.
#[test]
fn a_level_the_lane_does_not_offer_cannot_be_prepared_ahead() {
    let (mut lane, log) = sketch_lane(false);

    let prepared = prepare_ahead(
        &mut lane,
        SKETCH_LEVELS.len(),
        Some(target(key(TextureFormat::Rgba8Unorm))),
    );

    assert!(
        matches!(
            prepared,
            Err(NotPrepared::Failed(LaneError::InvalidInput { .. }))
        ),
        "{prepared:?}"
    );
    assert!(log.lock().unwrap().is_empty());
}
