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

//! A cost may say how much running its level on this frame's inputs changes
//! the result: a lane that cannot honour its level on some frames (a forward
//! lane past its light caps) quotes a worse impact than its level's. Saying
//! nothing means the level's own impact.

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::{FrameId, InputSource};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneBus, LaneError, LaneKind};
use khora_core::Runtime;
use khora_macros::{LaneInputs, LaneOutputs};

use crate::fixtures::{level, Positions};

/// **A cost names no impact unless its lane does.** The default cost, and so
/// every cost a lane builds from it without naming one, leaves the impact to
/// the level.
#[test]
fn a_cost_names_no_impact_unless_its_lane_does() {
    assert_eq!(Cost::default().impact, None);
}

/// What the capped lane writes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Shaded(pub u32);

/// The capped family's outputs.
#[derive(LaneOutputs)]
pub struct ShadedOutputs {
    #[deck]
    pub shaded: Shaded,
}

/// A family whose one lane can shade only so many lights.
pub struct CappedFamily;

impl LaneFamily for CappedFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "capped";
    const AGENT: AgentId = AgentId::new("renderer");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ShadedOutputs;
}

/// The lights to shade, one per position.
#[derive(LaneInputs)]
pub struct Lights<'f> {
    #[bus]
    pub lights: &'f Positions,
}

const CAPPED_LEVELS: &[Level] = &[level(StrategyId::Balanced, "two lights")];

/// Shades at most two lights: past them, its result differs a little.
#[derive(Default)]
pub struct CappedLane;

impl Lane for CappedLane {
    type Family = CappedFamily;
    type Inputs<'f> = Lights<'f>;

    fn levels(&self) -> &[Level] {
        CAPPED_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &Lights<'_>) -> Cost {
        Cost {
            impact: (inputs.lights.0.len() > 2).then_some(Impact::Minor),
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut Lights<'_>,
        out: &mut ShadedOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.shaded = Shaded(1);
        Ok(LaneReport::default())
    }
}

/// The quote of the capped lane over `lights` lights.
fn quote_over(lights: usize) -> Vec<Cost> {
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(1));
    bus.publish(Positions(vec![0.0; lights]));
    let runtime = Runtime::new();
    let mut inputs = InputSource::new(FrameId(1), &bus, &runtime);
    Erased::new(CappedLane)
        .quote(&mut inputs)
        .expect("the capped lane is quoted")
}

/// **A quote carries the impact its lane named, frame by frame.** Within its
/// caps the lane names none, so its level's impact stands; past them its
/// quote says `Minor`, whatever its level says.
#[test]
fn a_quote_carries_the_impact_its_lane_named() {
    assert_eq!(quote_over(2)[0].impact, None, "within the caps");
    assert_eq!(
        quote_over(3)[0].impact,
        Some(Impact::Minor),
        "past the caps"
    );
}
