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

//! The compile family: what its lanes stage, and who negotiates for it.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::{LaneKind, Leases};
use khora_macros::LaneOutputs;
use khora_script::reload::ScriptReload;
use khora_script::toolchain::ScriptDeclarations;

/// The family whose lane recompiles a project's scripts off the frame.
pub struct CompileFamily;

impl LaneFamily for CompileFamily {
    const KIND: LaneKind = LaneKind::Asset;
    const NAME: &'static str = "compile";
    const AGENT: AgentId = AgentId::new("compile");
    const SEMANTICS: Semantics = Semantics::Exact;
    type Outputs<'f> = CompileOutputs;
}

/// What a compile hands on, staged for a frame's boundary: the scripting
/// family's intake installs the programs, `script_components` registers the
/// declarations.
#[derive(LaneOutputs)]
pub struct CompileOutputs {
    /// The modules recompiled or withdrawn, each root's under the lease its
    /// compile holds to.
    #[staged]
    pub reloads: Leases<ScriptReload>,
    /// What the recompiled modules declare now, under the same leases.
    #[staged]
    pub declarations: Leases<ScriptDeclarations>,
}

// The family's mailbox: the requests the pump files, taken by its lane. Held
// while the family is off — a save is not forgotten because the compile
// waited.
khora_script::inventory::submit! {
    khora_core::event::Mailbox::of::<khora_script::toolchain::CompileRequest>(
        <CompileFamily as LaneFamily>::AGENT,
        khora_core::event::Reader::Take,
        khora_core::event::BetweenSessions::Hold,
    )
}
