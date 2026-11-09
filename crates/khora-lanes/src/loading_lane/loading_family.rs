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

//! The `loading` family: what its lanes stage, and who negotiates for it.

use khora_core::agent::gorna::AgentId;
use khora_core::asset::{Decoded, IndexedAssets};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::Leases;
use khora_data::scene::loading::PreparedLoad;
use khora_macros::LaneOutputs;
use khora_script::reload::ScriptReload;
use khora_script::toolchain::{CompileDone, ScriptDeclarations};

/// The family whose lanes load scenes, scripts and assets off the frame.
pub struct LoadingFamily;

impl LaneFamily for LoadingFamily {
    const NAME: &'static str = "loading";
    const AGENT: AgentId = AgentId::new("loading");
    const SEMANTICS: Semantics = Semantics::Exact;
    type Outputs<'f> = LoadingOutputs;
}

/// What a load hands on, staged for a frame's boundary: each landing reads
/// its own output by type.
#[derive(LaneOutputs)]
pub struct LoadingOutputs {
    /// One per project indexed, each under the lease of its project staying
    /// the one open.
    #[staged]
    pub indexed: Leases<IndexedAssets>,
    /// One per asset decoded, each under the lease of its asset's generation.
    #[staged]
    pub decoded: Leases<Decoded>,
    /// One per scene load prepared, each under the lease of the world's
    /// components staying as they were when it was prepared.
    #[staged]
    pub prepared: Leases<PreparedLoad>,
    /// The modules recompiled or withdrawn, each root's under the lease its
    /// compile holds to.
    #[staged]
    pub reloads: Leases<ScriptReload>,
    /// What the recompiled modules declare now, under the same leases.
    #[staged]
    pub declarations: Leases<ScriptDeclarations>,
    /// One per root compiled, carrying the tickets of every request merged
    /// into its compile, under the same lease as what it compiled.
    #[staged]
    pub compiled: Leases<CompileDone>,
}

// The family's mailboxes: what hosts, the hot-reload pump and the asset
// service file, taken by its lanes. Held while the family is off — a request
// filed is never forgotten because its load waited.
khora_script::inventory::submit! {
    khora_core::event::Mailbox::of::<khora_data::scene::loading::SceneLoadRequest>(
        <LoadingFamily as LaneFamily>::AGENT,
        khora_core::event::Reader::Take,
        khora_core::event::BetweenSessions::Hold,
    )
}
khora_script::inventory::submit! {
    khora_core::event::Mailbox::of::<khora_script::toolchain::CompileRequest>(
        <LoadingFamily as LaneFamily>::AGENT,
        khora_core::event::Reader::Take,
        khora_core::event::BetweenSessions::Hold,
    )
}
khora_script::inventory::submit! {
    khora_core::event::Mailbox::of::<khora_core::asset::IndexRequest>(
        <LoadingFamily as LaneFamily>::AGENT,
        khora_core::event::Reader::Take,
        khora_core::event::BetweenSessions::Hold,
    )
}
khora_script::inventory::submit! {
    khora_core::event::Mailbox::of::<khora_core::asset::DecodeRequest>(
        <LoadingFamily as LaneFamily>::AGENT,
        khora_core::event::Reader::Take,
        khora_core::event::BetweenSessions::Hold,
    )
}
