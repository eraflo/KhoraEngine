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
    /// A safepoint costs nothing: it marks a place rather than doing work. An
    /// instruction that builds, writes or compares `count` values pays for each:
    /// a ten-thousand-element literal is not one unit of work. A `Become` pays one
    /// more for each field it writes; a `Raise` pays a flat 4. An operation whose
    /// size only the running program knows — `Copy`, a field read or write of an
    /// array — is charged by the VM when it runs; this is its floor.
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
            // Queuing an event is priced as an engine function doing it would be.
            Self::Raise { .. } => 4,
            _ => 1,
        }
    }
}
