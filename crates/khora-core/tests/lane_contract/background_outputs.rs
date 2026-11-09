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

//! What a background lane may hand on: its `#[staged]` outputs alone. A deck
//! slot is one frame's, and a run spanning frames would write it after that
//! frame ended.

use std::any::TypeId;

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::{check_registrations, AgentRegistration, BootError};
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::SinkKind;
use khora_core::lane::level::Level;
use khora_core::lane::LaneError;
use khora_core::lane::{Lane, LaneReport};
use khora_core::Runtime;
use khora_macros::{LaneInputs, LaneOutputs};

use crate::fixtures::{level, Contacts};

const LEVELS: &[Level] = &[level(StrategyId::Balanced, "only")];

/// Reads nothing.
#[derive(LaneInputs)]
pub struct NoInputs {}

/// Writes one deck slot.
#[derive(LaneOutputs)]
pub struct WritesDeck {
    #[deck]
    pub contacts: Contacts,
}

/// A family whose lanes write a deck slot.
pub struct DeckWritingFamily;

impl LaneFamily for DeckWritingFamily {
    const NAME: &'static str = "deck_writing";
    const AGENT: AgentId = AgentId::new("deck_writing");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = WritesDeck;
}

#[derive(Default)]
pub struct DeckWritingLane;

impl Lane for DeckWritingLane {
    type Family = DeckWritingFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        _out: &mut WritesDeck,
    ) -> Result<LaneReport, LaneError> {
        Ok(LaneReport::default())
    }
}

fn deck_writing_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(DeckWritingLane))
}

/// **Boot refuses a background lane writing a deck slot**, naming the lane,
/// the field and where it writes; the same lane in a frame phase boots.
#[test]
fn boot_refuses_a_background_lane_writing_a_slot_of_one_frame() {
    let registration = |phase| AgentRegistration {
        family: TypeId::of::<DeckWritingFamily>(),
        family_name: DeckWritingFamily::NAME,
        agent: DeckWritingFamily::AGENT,
        semantics: DeckWritingFamily::SEMANTICS,
        phase,
        importance: AgentImportance::Optional,
        after: &[],
        fixed_step: None,
        default_level: None,
        module: module_path!(),
        lanes: &[deck_writing_lane],
    };

    match check_registrations(
        &[&registration(ExecutionPhase::BACKGROUND)],
        &Runtime::new(),
        &DeviceCaps::default(),
    ) {
        Err(BootError::BackgroundOutput {
            family,
            lane,
            field,
            sink,
        }) => {
            assert_eq!(family, "deck_writing");
            assert!(
                lane.ends_with("DeckWritingLane"),
                "the lane is named: {lane}"
            );
            assert_eq!(field, "contacts");
            assert_eq!(sink, SinkKind::Deck);
        }
        other => panic!("expected BackgroundOutput, got {other:?}"),
    }

    assert_eq!(
        check_registrations(
            &[&registration(ExecutionPhase::OUTPUT)],
            &Runtime::new(),
            &DeviceCaps::default(),
        ),
        Ok(()),
        "in a frame phase, writing the deck is the lane's job"
    );
}
