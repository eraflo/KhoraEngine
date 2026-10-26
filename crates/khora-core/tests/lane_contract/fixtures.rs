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

//! The views, runtime entries, families and lanes the contract tests share.

use std::sync::{Arc, Mutex};

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{Validate, ValueProblem};
use khora_core::lane::io::{Budget, Locked};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{LaneError, LaneKind};
use khora_core::problems::ProblemSeverity;
use khora_macros::{LaneInputs, LaneOutputs};

// ─── Views (what Flows publish) ─────────────────────────────────────────────

/// A View: the positions a Flow projected.
#[derive(Debug, Clone, PartialEq)]
pub struct Positions(pub Vec<f32>);

/// A View that checks itself: a speed is never negative.
#[derive(Debug, Clone, PartialEq)]
pub struct Speeds {
    pub values: Vec<f32>,
}

impl Validate for Speeds {
    fn validate(&self) -> Vec<ValueProblem> {
        self.values
            .iter()
            .filter(|speed| **speed < 0.0)
            .map(|speed| ValueProblem {
                field: "values",
                severity: ProblemSeverity::Warning,
                message: format!("a speed is never negative (it is {speed})"),
            })
            .collect()
    }
}

// ─── Runtime entries ────────────────────────────────────────────────────────

/// A runtime resource, read without locking.
#[derive(Debug, Clone, PartialEq)]
pub struct MixSettings {
    pub gain: f32,
}

/// A runtime resource registered as `Arc<Mutex<Counter>>`, locked.
#[derive(Debug, Default)]
pub struct Counter(pub u32);

/// The type a `Locked<'f, Counter>` is registered as.
pub type SharedCounter = Arc<Mutex<Counter>>;

// ─── Deck slots ─────────────────────────────────────────────────────────────

/// A slot an earlier phase wrote.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Contacts(pub u32);

/// A slot the mix family writes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MixedFrames(pub u32);

/// A slot the speed family writes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Fastest(pub f32);

/// A slot the echo family writes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Echoes(pub u32);

// ─── Inputs ─────────────────────────────────────────────────────────────────

/// One field for each input source these tests can build.
#[derive(LaneInputs)]
pub struct EveryInput<'f> {
    #[bus]
    pub view: &'f Positions,
    #[deck]
    pub earlier: Option<&'f Contacts>,
    #[resource]
    pub settings: &'f MixSettings,
    #[locked]
    pub counter: Locked<'f, Counter>,
    #[budget]
    pub budget: Budget,
}

/// A view and a resource. Only ever fetched to be refused, so never read.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct ViewAndSettings<'f> {
    #[bus]
    pub view: &'f Positions,
    #[resource]
    pub settings: &'f MixSettings,
}

/// One view.
#[derive(LaneInputs)]
pub struct OneView<'f> {
    #[bus]
    pub view: &'f Positions,
}

/// One view its own check runs on.
#[derive(LaneInputs)]
pub struct CheckedSpeeds<'f> {
    #[bus(validate)]
    pub speeds: &'f Speeds,
}

/// What the mix lanes read.
#[derive(LaneInputs)]
pub struct MixInputs<'f> {
    #[resource]
    pub settings: &'f MixSettings,
}

/// What the echo lane reads: the mix family's output, from an earlier phase.
#[derive(LaneInputs)]
pub struct EchoInputs<'f> {
    #[deck]
    pub mixed: Option<&'f MixedFrames>,
}

// ─── Families and their outputs ─────────────────────────────────────────────

/// The speed family's outputs.
#[derive(LaneOutputs)]
pub struct SpeedOutputs {
    #[deck]
    pub fastest: Fastest,
}

/// The mix family's outputs.
#[derive(LaneOutputs)]
pub struct MixOutputs {
    #[deck]
    pub mixed: MixedFrames,
}

/// The echo family's outputs.
#[derive(LaneOutputs)]
pub struct EchoOutputs {
    #[deck]
    pub echoes: Echoes,
}

/// Writes the fastest speed it was given.
pub struct SpeedFamily;

impl LaneFamily for SpeedFamily {
    const KIND: LaneKind = LaneKind::Physics;
    const NAME: &'static str = "speed";
    const AGENT: AgentId = AgentId::Physics;
    const SEMANTICS: Semantics = Semantics::Exact;
    type Outputs<'f> = SpeedOutputs;
}

/// Mixes frames.
pub struct MixFamily;

impl LaneFamily for MixFamily {
    const KIND: LaneKind = LaneKind::Audio;
    const NAME: &'static str = "mix";
    const AGENT: AgentId = AgentId::Audio;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = MixOutputs;
}

/// Echoes what the mix family wrote.
pub struct EchoFamily;

impl LaneFamily for EchoFamily {
    const KIND: LaneKind = LaneKind::Ui;
    const NAME: &'static str = "echo";
    const AGENT: AgentId = AgentId::Ui;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = EchoOutputs;
}

// ─── Lanes ──────────────────────────────────────────────────────────────────

/// A level holding nothing, named by `id`.
pub const fn level(id: StrategyId, name: &'static str) -> Level {
    Level {
        id,
        name,
        impact: Impact::None,
        memory: PerDevice {
            cpu: 0,
            gpu: 0,
            npu: 0,
            other: 0,
        },
    }
}

const FASTEST_LEVELS: &[Level] = &[level(StrategyId::Balanced, "fastest")];
const MIX_LEVELS: &[Level] = &[level(StrategyId::Balanced, "mix")];
const CHEAP_MIX_LEVELS: &[Level] = &[level(StrategyId::LowPower, "cheap mix")];
const ECHO_LEVELS: &[Level] = &[level(StrategyId::Balanced, "echo")];

/// Records the fastest of the speeds it reads, negative ones included.
#[derive(Default)]
pub struct FastestLane;

impl Lane for FastestLane {
    type Family = SpeedFamily;
    type Inputs<'f> = CheckedSpeeds<'f>;

    fn levels(&self) -> &[Level] {
        FASTEST_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &CheckedSpeeds<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut CheckedSpeeds<'_>,
        out: &mut SpeedOutputs,
    ) -> Result<LaneReport, LaneError> {
        let fastest = inputs
            .speeds
            .values
            .iter()
            .copied()
            .fold(f32::MIN, f32::max);
        out.fastest = Fastest(fastest);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

/// Mixes at the `Balanced` level, reading `MixSettings`.
#[derive(Default)]
pub struct MixLane;

impl Lane for MixLane {
    type Family = MixFamily;
    type Inputs<'f> = MixInputs<'f>;

    fn levels(&self) -> &[Level] {
        MIX_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &MixInputs<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut MixInputs<'_>,
        out: &mut MixOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.mixed = MixedFrames(inputs.settings.gain.round() as u32);
        Ok(LaneReport::default())
    }
}

/// Mixes at the `LowPower` level, reading `MixSettings`.
#[derive(Default)]
pub struct CheapMixLane;

impl Lane for CheapMixLane {
    type Family = MixFamily;
    type Inputs<'f> = MixInputs<'f>;

    fn levels(&self) -> &[Level] {
        CHEAP_MIX_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &MixInputs<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut MixInputs<'_>,
        out: &mut MixOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.mixed = MixedFrames(1);
        Ok(LaneReport::default())
    }
}

/// Also mixes at the `Balanced` level: offered beside `MixLane`, the mix
/// family has two rows named `Balanced`.
#[derive(Default)]
pub struct OtherBalancedMixLane;

impl Lane for OtherBalancedMixLane {
    type Family = MixFamily;
    type Inputs<'f> = MixInputs<'f>;

    fn levels(&self) -> &[Level] {
        MIX_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &MixInputs<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut MixInputs<'_>,
        out: &mut MixOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.mixed = MixedFrames(2);
        Ok(LaneReport::default())
    }
}

/// Echoes the frames the mix family wrote in an earlier phase.
#[derive(Default)]
pub struct EchoLane;

impl Lane for EchoLane {
    type Family = EchoFamily;
    type Inputs<'f> = EchoInputs<'f>;

    fn levels(&self) -> &[Level] {
        ECHO_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &EchoInputs<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut EchoInputs<'_>,
        out: &mut EchoOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.echoes = Echoes(inputs.mixed.map_or(0, |mixed| mixed.0));
        Ok(LaneReport::default())
    }
}
