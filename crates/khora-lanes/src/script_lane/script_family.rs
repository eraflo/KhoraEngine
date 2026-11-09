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

//! The script family: the lane that runs the frame's behaviours within its
//! budget, and what every one of its lanes hands on.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::script::{BehaviorWriteback, CommandBuffer};
use khora_macros::LaneOutputs;

/// The script family. Its output is simulation: a cut defers a turn, never
/// changes what a turn computes, so every level keeps the result.
pub struct ScriptFamily;

impl LaneFamily for ScriptFamily {
    const NAME: &'static str = "script";
    const AGENT: AgentId = khora_core::script::SCRIPT;
    const SEMANTICS: Semantics = Semantics::Exact;
    type Outputs<'f> = ScriptOutputs;
}

/// What every script lane writes: the commands its behaviours queued, applied
/// at the frame boundary, and the instance state the scene records.
#[derive(LaneOutputs)]
pub struct ScriptOutputs {
    /// The commands the frame's behaviours queued.
    #[deck]
    pub commands: CommandBuffer,
    /// The state of the instances that did work, for the scene to record.
    #[deck]
    pub writeback: BehaviorWriteback,
}
