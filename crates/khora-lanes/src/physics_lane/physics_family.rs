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

//! The physics family: the lanes that step the simulation, and what every one
//! of them hands on.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::LaneKind;
use khora_core::physics::ContactBatch;
use khora_data::flow::PhysicsStepResult;
use khora_macros::LaneOutputs;

/// The physics family. Its output is simulation: every level gives the same
/// result, so boot refuses a level whose impact is not `None`.
pub struct PhysicsFamily;

impl LaneFamily for PhysicsFamily {
    const KIND: LaneKind = LaneKind::Physics;
    const NAME: &'static str = "physics";
    const AGENT: AgentId = AgentId::Physics;
    const SEMANTICS: Semantics = Semantics::Exact;
    type Outputs<'f> = PhysicsOutputs;
}

/// What every physics lane writes: the step it advanced, drained by the
/// `physics_world_writeback` DataSystem, and the contacts it reported,
/// drained by `collision_dispatch`.
#[derive(LaneOutputs)]
pub struct PhysicsOutputs {
    /// The step the simulation advanced.
    #[deck]
    pub step: PhysicsStepResult,
    /// The contacts the step reported, each naming two entities.
    #[deck]
    pub contacts: ContactBatch,
}
