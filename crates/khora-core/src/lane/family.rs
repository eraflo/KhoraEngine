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

//! A lane family: the outputs several behaviours share.
//!
//! Two lanes belong to the same family when they produce the same output with
//! the same meaning for its readers (Forward, Forward+ and PBR all draw the
//! scene). One agent runs one family; its lanes are the behaviours it chooses
//! between.

use super::io::LaneOutputs;
use super::LaneKind;
use crate::agent::gorna::AgentId;

/// What a family's adaptation may change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Semantics {
    /// The output is presentation: a cheaper level may look worse.
    Presentation,
    /// The output is simulation: every level must give the same result.
    Exact,
}

/// The outputs every lane of one family writes, and who negotiates for them.
pub trait LaneFamily: 'static {
    /// The kind of lane the family holds.
    const KIND: LaneKind;
    /// The family's name, as boot errors and telemetry name it.
    const NAME: &'static str;
    /// The agent that negotiates for the family.
    const AGENT: AgentId;
    /// What adaptation may change in the family's outputs.
    const SEMANTICS: Semantics;
    /// The outputs every lane of the family writes.
    type Outputs<'f>: LaneOutputs<'f>;
}
