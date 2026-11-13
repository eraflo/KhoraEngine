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

//! What running one behavior's turn did: how it ended, and what it cost.

/// What running one behavior did.
pub(super) enum Outcome {
    /// A body stopped part-way and is kept in [`Progress::pending`] for a
    /// later turn.
    ///
    /// [`Progress::pending`]: super::progress::Progress::pending
    Busy {
        spent: u64,
        /// How many of this instance's events it got through first.
        delivered: usize,
        /// Whether it stopped because the budget ran out, rather than on an
        /// `await`. A starved turn is budget pressure the DCC must see; a
        /// waiting one is a script doing what it says.
        starved: bool,
    },
    Completed {
        spent: u64,
    },
    Deferred {
        spent: u64,
        /// How many of this instance's events were handled before the fuel ran
        /// out.
        ///
        /// The rest were never seen, and the frame puts them back. Counting
        /// rather than returning them keeps `Outcome` free of a borrow, and
        /// counting rather than putting *all* of them back is the difference
        /// between a `Damaged` arriving late and arriving twice.
        delivered: usize,
    },
    Faulted {
        spent: u64,
        reason: String,
    },
}

impl Outcome {
    /// A fault, with what had been spent when it happened.
    ///
    /// Most of the ways a turn can fault format the same `Fault` the same way,
    /// and each one has to remember to carry `spent` — a turn whose cost is
    /// dropped is fuel the frame never accounts for and the DCC never sees.
    pub(super) fn faulted(spent: u64, fault: impl std::fmt::Debug) -> Self {
        Self::Faulted {
            spent,
            reason: format!("{fault:?}"),
        }
    }

    /// What it cost, however it ended.
    pub(super) fn spent(&self) -> u64 {
        match self {
            Self::Completed { spent }
            | Self::Deferred { spent, .. }
            | Self::Busy { spent, .. }
            | Self::Faulted { spent, .. } => *spent,
        }
    }
}
