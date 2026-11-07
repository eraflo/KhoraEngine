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

//! The decode family: what its lanes stage, and who negotiates for it.

use khora_core::agent::gorna::AgentId;
use khora_core::asset::Decoded;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::{LaneKind, Leases};
use khora_macros::LaneOutputs;

/// The family whose lane decodes assets off the frame.
pub struct DecodeFamily;

impl LaneFamily for DecodeFamily {
    const KIND: LaneKind = LaneKind::Asset;
    const NAME: &'static str = "decode";
    const AGENT: AgentId = AgentId::new("decode");
    const SEMANTICS: Semantics = Semantics::Exact;
    type Outputs<'f> = DecodeOutputs;
}

/// What a decode hands on, staged for a frame's boundary: the asset intake
/// installs each value.
#[derive(LaneOutputs)]
pub struct DecodeOutputs {
    /// One per request, each under the lease of its asset's generation.
    #[staged]
    pub decoded: Leases<Decoded>,
}

// The family's mailbox: the requests the asset service files, taken by its
// lane. Held while the family is off — an asset asked for is not forgotten
// because its decode waited.
khora_script::inventory::submit! {
    khora_core::event::Mailbox::of::<khora_core::asset::DecodeRequest>(
        <DecodeFamily as LaneFamily>::AGENT,
        khora_core::event::Reader::Take,
        khora_core::event::BetweenSessions::Hold,
    )
}
