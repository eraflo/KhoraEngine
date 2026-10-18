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

//! What each instruction costs from the fuel budget, and where a run that
//! cannot pay for one stops.

use super::instruction::Instruction;

impl Instruction {
    /// Where a run stops when it cannot pay for this one, when that is not
    /// the instruction itself.
    pub fn stops_at(&self) -> Option<usize> {
        match self {
            Self::RemoveAt {
                charged: true,
                from,
                ..
            } => Some(*from),
            _ => None,
        }
    }

    /// What running this costs from the fuel budget.
    ///
    /// Uniform today, except for a safepoint, which marks a place rather than
    /// doing work. It is a method rather than a constant because a call and a
    /// move plainly do not cost the same, and the shape should be here when
    /// measurement says so — not retrofitted through every call site.
    ///
    /// Building an array pays for its elements: a ten-thousand-element literal
    /// is not one unit of work. An operation whose size only the running
    /// program knows — `Copy`, a field read or write of an array — is charged
    /// by the VM when it runs; this is its floor.
    pub fn cost(&self) -> u64 {
        match self {
            Self::Safepoint => 0,
            Self::NewArray { count, .. }
            | Self::Extend { count, .. }
            | Self::NewStruct { count, .. }
            | Self::WriteComponent { count, .. }
            | Self::GetComponent { count, .. }
            | Self::QueryHolds { count, .. }
            | Self::WriteRow { count, .. }
            | Self::Format { count, .. }
            | Self::SpawnEntity { count, .. }
            | Self::SpawnPrefab { count, .. } => 1 + u64::from(*count),
            Self::Become { writes, .. } => 1 + u64::from(*writes),
            // What the engine function it replaced was charged.
            Self::Raise { .. } => 4,
            _ => 1,
        }
    }
}
