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

//! An output field that could not be committed, and where it was to be written.

use super::guard::Reason;

/// An output field that could not be committed, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputRefusal {
    /// The field's name in the `LaneOutputs` struct.
    pub field: &'static str,
    /// Where it is written.
    pub sink: SinkKind,
    /// Why it was refused.
    pub reason: Reason,
}

/// Where an output field is written — one variant per `LaneOutputs` field
/// attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SinkKind {
    /// `#[pass]`: GPU commands for a pass slot.
    Pass,
    /// `#[deck]`: CPU data for another domain.
    Deck,
    /// `#[staged]`: outputs kept in the family's reserve across frames,
    /// applied at a frame's `Boundary`.
    Staged,
}
