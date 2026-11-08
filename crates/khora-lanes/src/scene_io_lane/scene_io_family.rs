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

//! The `scene_io` family: what its lane stages, and who negotiates for it.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::{LaneKind, Leases};
use khora_data::scene::loading::PreparedLoad;
use khora_macros::LaneOutputs;

/// The family whose lane prepares scene loads off the frame.
pub struct SceneIoFamily;

impl LaneFamily for SceneIoFamily {
    const KIND: LaneKind = LaneKind::Asset;
    const NAME: &'static str = "scene_io";
    const AGENT: AgentId = AgentId::new("scene_io");
    const SEMANTICS: Semantics = Semantics::Exact;
    type Outputs<'f> = SceneIoOutputs;
}

/// What a scene load hands on, staged for a frame's boundary: the scene
/// intake commits each.
#[derive(LaneOutputs)]
pub struct SceneIoOutputs {
    /// One per request, each under the lease of the world's components
    /// staying as they were when it was prepared.
    #[staged]
    pub prepared: Leases<PreparedLoad>,
}

// The family's mailbox: the scene loads hosts file, taken by its lane. Held
// while the family is off — a load filed is never forgotten.
khora_script::inventory::submit! {
    khora_core::event::Mailbox::of::<khora_data::scene::loading::SceneLoadRequest>(
        <SceneIoFamily as LaneFamily>::AGENT,
        khora_core::event::Reader::Take,
        khora_core::event::BetweenSessions::Hold,
    )
}
